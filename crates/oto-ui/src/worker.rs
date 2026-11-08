//! DB thread: owns the `Package`; the UI talks to it through channels so SQLite writes never
//! block a frame. Consecutive saves are coalesced (only the newest state is written).

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver, Sender};
use oto_core::media::MediaItem;
use oto_core::model::Project;
use oto_store::{Package, SnapshotInfo};

pub enum StoreReq {
    Save { project: Box<Project>, media: Arc<Vec<MediaItem>>, kind: &'static str, revision: u64 },
    Snapshot { reason: String, manual: bool },
    ListSnapshots,
    Restore(PathBuf),
    RecordExport { kind: String, path: PathBuf, preset: String, audit: String, checksums: String },
    RecordProof { path: PathBuf, note: String },
    SaveTemplate { id: String, toml: String, version: u32 },
    SaveLab { id: String, toml: String },
    SaveAs { path: PathBuf, project: Box<Project>, media: Arc<Vec<MediaItem>> },
    Close,
}

pub enum StoreResp {
    Saved { revision: u64, kind: &'static str },
    SaveFailed(String),
    Snapshot(Result<SnapshotInfo, String>),
    Snapshots(Vec<SnapshotInfo>),
    Restored(Result<Box<(Project, Vec<MediaItem>)>, String>),
    SavedAs(Result<PathBuf, String>),
    Closed,
    Error(String),
}

pub struct StoreWorker {
    pub tx: Sender<StoreReq>,
    pub rx: Receiver<StoreResp>,
}

impl StoreWorker {
    pub fn spawn(pkg: Package) -> StoreWorker {
        let (tx, req_rx) = unbounded::<StoreReq>();
        let (resp_tx, rx) = unbounded::<StoreResp>();
        let res = std::thread::Builder::new().name("oto-db".into()).spawn(move || run(pkg, req_rx, resp_tx));
        if let Err(e) = res {
            tracing::error!("cannot start DB thread: {e}");
        }
        StoreWorker { tx, rx }
    }

    pub fn send(&self, r: StoreReq) {
        if self.tx.send(r).is_err() {
            tracing::error!("DB thread is gone");
        }
    }
}

fn run(mut pkg: Package, rx: Receiver<StoreReq>, tx: Sender<StoreResp>) {
    let mut pending: Vec<StoreReq> = Vec::new();
    loop {
        let first = match pending.pop() {
            Some(r) => r,
            None => match rx.recv() {
                Ok(r) => r,
                Err(_) => break,
            },
        };
        // Coalesce queued saves: keep only the newest one, but preserve other requests in order.
        let mut req = first;
        if matches!(req, StoreReq::Save { .. }) {
            let mut others = Vec::new();
            while let Ok(next) = rx.try_recv() {
                if matches!(next, StoreReq::Save { .. }) {
                    req = next;
                } else {
                    others.push(next);
                }
            }
            others.reverse();
            pending.extend(others);
        }
        match req {
            StoreReq::Save { project, media, kind, revision } => {
                let r = match pkg.save(&project, &media, kind, revision) {
                    Ok(()) => StoreResp::Saved { revision, kind },
                    Err(e) => StoreResp::SaveFailed(e.to_string()),
                };
                let _ = tx.send(r);
            }
            StoreReq::Snapshot { reason, manual } => {
                let _ = tx.send(StoreResp::Snapshot(pkg.snapshot(&reason, manual).map_err(|e| e.to_string())));
            }
            StoreReq::ListSnapshots => {
                let _ = tx.send(StoreResp::Snapshots(pkg.list_snapshots().unwrap_or_default()));
            }
            StoreReq::Restore(path) => {
                let r = pkg.restore_snapshot(&path).and_then(|_| pkg.load()).map_err(|e| e.to_string());
                let _ = tx.send(StoreResp::Restored(r.map(Box::new)));
            }
            StoreReq::RecordExport { kind, path, preset, audit, checksums } => {
                if let Err(e) = pkg.record_export(&kind, &path, &preset, &audit, &checksums) {
                    let _ = tx.send(StoreResp::Error(e.to_string()));
                }
            }
            StoreReq::RecordProof { path, note } => {
                if let Err(e) = pkg.record_proof(&path, &note) {
                    let _ = tx.send(StoreResp::Error(e.to_string()));
                }
            }
            StoreReq::SaveTemplate { id, toml, version } => {
                if let Err(e) = pkg.save_template(&id, &toml, version) {
                    let _ = tx.send(StoreResp::Error(e.to_string()));
                }
            }
            StoreReq::SaveLab { id, toml } => {
                if let Err(e) = pkg.save_lab_profile(&id, &toml) {
                    let _ = tx.send(StoreResp::Error(e.to_string()));
                }
            }
            StoreReq::SaveAs { path, project, media } => match pkg.save_as(&path, &project, &media) {
                Ok(new_pkg) => {
                    let root = new_pkg.root.clone();
                    let old = std::mem::replace(&mut pkg, new_pkg);
                    let _ = old.close();
                    let _ = tx.send(StoreResp::SavedAs(Ok(root)));
                }
                Err(e) => {
                    let _ = tx.send(StoreResp::SavedAs(Err(e.to_string())));
                }
            },
            StoreReq::Close => {
                let r = pkg.close();
                if let Err(e) = r {
                    tracing::error!("close: {e}");
                }
                let _ = tx.send(StoreResp::Closed);
                return;
            }
        }
    }
}
