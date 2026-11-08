//! oto-store: the `.otoalbum` package (SQLite source of truth + JSON manifest), migrations,
//! autosave, snapshots and crash recovery.
//!
//! ```text
//! MyAlbum.otoalbum/
//!   project.sqlite   manifest.json   media/links.json
//!   cache/{thumbs,proxies,previews}  snapshots/  exports/  logs/
//! ```
#![forbid(unsafe_code)]

pub mod sample;

use std::fs;
use std::path::{Path, PathBuf};

use oto_core::geometry::RectUm;
use oto_core::media::{ColorLabel, MediaItem};
use oto_core::model::*;
use oto_core::spec::{AlbumSpec, Binding, ColorProfile, CoverType};
use oto_core::units::{Um, Units};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DB_SCHEMA_VERSION: u32 = 2;
pub const PACKAGE_EXT: &str = "otoalbum";
pub const AUTO_SNAPSHOTS_KEPT: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("file error at {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("data error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} is not an oto-albums project (missing project.sqlite)")]
    NotAProject(String),
    #[error("project database is damaged ({0}); restore a snapshot")]
    Corrupt(String),
    #[error("project was made by a newer oto-albums (schema {0})")]
    TooNew(u32),
    #[error("project has no project row")]
    Empty,
}

fn io(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io { path: path.display().to_string(), source }
}

pub type Result<T> = std::result::Result<T, StoreError>;

// ---------------------------------------------------------------------------------------------
// Schema & migrations
// ---------------------------------------------------------------------------------------------

const SQL_V1: &str = r#"
CREATE TABLE IF NOT EXISTS migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, client TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, due_date TEXT, status TEXT NOT NULL,
  lab_id TEXT, lab_size_id TEXT, settings_json TEXT NOT NULL, staged_json TEXT NOT NULL DEFAULT '[]',
  model_schema INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS album_specs (
  project_id TEXT PRIMARY KEY, page_width_um INTEGER NOT NULL, page_height_um INTEGER NOT NULL,
  spread_width_um INTEGER NOT NULL, spread_height_um INTEGER NOT NULL, bleed_um INTEGER NOT NULL,
  trim_outer_um INTEGER NOT NULL, trim_inside_um INTEGER NOT NULL, safe_um INTEGER NOT NULL,
  gutter_um INTEGER NOT NULL, min_pages INTEGER NOT NULL, max_pages INTEGER NOT NULL,
  page_increment INTEGER NOT NULL, units TEXT NOT NULL, cover_type TEXT NOT NULL,
  binding TEXT NOT NULL, target_dpi INTEGER NOT NULL, profile_name TEXT NOT NULL, profile_icc TEXT
);
CREATE TABLE IF NOT EXISTS lab_profiles (id TEXT PRIMARY KEY, toml TEXT NOT NULL, added_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS media (
  id TEXT PRIMARY KEY, path TEXT NOT NULL, file_name TEXT NOT NULL, hash TEXT NOT NULL,
  size INTEGER NOT NULL, mtime INTEGER NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL,
  orientation INTEGER NOT NULL, format TEXT NOT NULL, capture_time TEXT, camera TEXT, lens TEXT,
  iso INTEGER, aperture REAL, shutter TEXT, focal_mm REAL, color_profile TEXT,
  rating INTEGER NOT NULL DEFAULT 0, color_label TEXT NOT NULL DEFAULT 'none',
  rejected INTEGER NOT NULL DEFAULT 0, flagged INTEGER NOT NULL DEFAULT 0, client_pick INTEGER NOT NULL DEFAULT 0,
  bins_json TEXT NOT NULL DEFAULT '[]', missing INTEGER NOT NULL DEFAULT 0, features_json TEXT NOT NULL,
  similar_group INTEGER, imported_at TEXT NOT NULL, sort_index INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS media_hash ON media(hash);
CREATE TABLE IF NOT EXISTS spreads (
  id TEXT PRIMARY KEY, idx INTEGER NOT NULL, is_cover INTEGER NOT NULL DEFAULT 0,
  template_id TEXT NOT NULL, background_json TEXT NOT NULL, locked INTEGER NOT NULL DEFAULT 0,
  notes TEXT NOT NULL DEFAULT '', surface_w_um INTEGER NOT NULL, surface_h_um INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS frames (
  id TEXT PRIMARY KEY, spread_id TEXT NOT NULL, ord INTEGER NOT NULL, z INTEGER NOT NULL,
  x_um INTEGER NOT NULL, y_um INTEGER NOT NULL, w_um INTEGER NOT NULL, h_um INTEGER NOT NULL,
  image_id TEXT, crop_cx REAL NOT NULL, crop_cy REAL NOT NULL, crop_zoom REAL NOT NULL,
  crop_rot_mdeg INTEGER NOT NULL, crop_manual INTEGER NOT NULL, focus_json TEXT,
  rotation_mdeg INTEGER NOT NULL, opacity REAL NOT NULL, border_json TEXT NOT NULL, radius_mm REAL NOT NULL,
  fit TEXT NOT NULL, adjust_json TEXT NOT NULL, locked INTEGER NOT NULL, allow_rotate INTEGER NOT NULL, role TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS texts (
  id TEXT PRIMARY KEY, spread_id TEXT NOT NULL, ord INTEGER NOT NULL, z INTEGER NOT NULL,
  x_um INTEGER NOT NULL, y_um INTEGER NOT NULL, w_um INTEGER NOT NULL, h_um INTEGER NOT NULL,
  body TEXT NOT NULL, style_json TEXT NOT NULL, rotation_mdeg INTEGER NOT NULL, locked INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS shapes (
  id TEXT PRIMARY KEY, spread_id TEXT NOT NULL, ord INTEGER NOT NULL, z INTEGER NOT NULL,
  x_um INTEGER NOT NULL, y_um INTEGER NOT NULL, w_um INTEGER NOT NULL, h_um INTEGER NOT NULL,
  kind TEXT NOT NULL, style_json TEXT NOT NULL, rotation_mdeg INTEGER NOT NULL, locked INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS assets (id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT NOT NULL, hash TEXT);
CREATE TABLE IF NOT EXISTS templates (id TEXT PRIMARY KEY, toml TEXT NOT NULL, version INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS snapshots (id TEXT PRIMARY KEY, path TEXT NOT NULL, reason TEXT NOT NULL, created_at TEXT NOT NULL, manual INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS exports (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, kind TEXT NOT NULL, path TEXT NOT NULL, preset TEXT NOT NULL, audit_json TEXT NOT NULL, checksums_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS proof_revisions (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, path TEXT NOT NULL, note TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS comments (
  id TEXT PRIMARY KEY, revision_id TEXT, spread_id TEXT NOT NULL, x_norm REAL NOT NULL, y_norm REAL NOT NULL,
  image_id TEXT, author TEXT NOT NULL, body TEXT NOT NULL, created_at TEXT NOT NULL, resolved_at TEXT
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

/// v2: Auto Build explanations on spreads, user focus points on media, and a commit journal.
const SQL_V2: &str = r#"
ALTER TABLE spreads ADD COLUMN explain TEXT NOT NULL DEFAULT '';
ALTER TABLE media ADD COLUMN focus_json TEXT;
CREATE TABLE IF NOT EXISTS journal (seq INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, kind TEXT NOT NULL, revision INTEGER NOT NULL, note TEXT NOT NULL DEFAULT '');
"#;

const MIGRATIONS: [(u32, &str); 2] = [(1, SQL_V1), (2, SQL_V2)];

fn current_version(conn: &Connection) -> Result<u32> {
    let has: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='migrations'", [], |r| r.get::<_, i64>(0))? > 0;
    if !has {
        return Ok(0);
    }
    Ok(conn.query_row("SELECT COALESCE(MAX(version),0) FROM migrations", [], |r| r.get::<_, i64>(0))? as u32)
}

/// Apply migrations up to `target` (idempotent). Returns the version before migrating.
pub fn migrate_to(conn: &mut Connection, target: u32) -> Result<u32> {
    let from = current_version(conn)?;
    if from > DB_SCHEMA_VERSION {
        return Err(StoreError::TooNew(from));
    }
    for (v, sql) in MIGRATIONS {
        if v > from && v <= target {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(sql)?;
            tx.execute("INSERT OR REPLACE INTO migrations(version, applied_at) VALUES (?1, ?2)", params![v, now_iso()])?;
            tx.commit()?;
        }
    }
    Ok(from)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Package
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub schema_version: u32,
    pub app_version: String,
    pub project_id: String,
    pub name: String,
    pub created_at: String,
    pub modified_at: String,
    pub spec: ManifestSpec,
    pub lab_profile: Option<String>,
    pub spread_count: usize,
    pub page_count: u32,
    pub cover: bool,
    pub media_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestSpec {
    pub page_width_mm: f64,
    pub page_height_mm: f64,
    pub spread_width_mm: f64,
    pub bleed_mm: f64,
    pub safe_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaLink {
    pub id: String,
    pub path: String,
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub id: String,
    pub path: PathBuf,
    pub reason: String,
    pub created_at: String,
    pub manual: bool,
}

#[derive(Debug, Clone, Default)]
pub struct OpenReport {
    /// The previous session did not close cleanly (crash / kill).
    pub unclean_shutdown: bool,
    /// Time of the last committed autosave.
    pub last_commit_at: Option<String>,
    pub migrated_from: Option<u32>,
}

pub struct Package {
    pub root: PathBuf,
    conn: Connection,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package").field("root", &self.root).finish()
    }
}

pub fn ensure_ext(p: &Path) -> PathBuf {
    if p.extension().and_then(|e| e.to_str()) == Some(PACKAGE_EXT) {
        p.to_path_buf()
    } else {
        let mut s = p.as_os_str().to_owned();
        s.push(".");
        s.push(PACKAGE_EXT);
        PathBuf::from(s)
    }
}

impl Package {
    pub fn db_path(root: &Path) -> PathBuf {
        root.join("project.sqlite")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn exports_dir(&self) -> PathBuf {
        self.root.join("exports")
    }
    pub fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    fn make_dirs(root: &Path) -> Result<()> {
        for d in ["media", "cache/thumbs", "cache/proxies", "cache/previews", "snapshots", "exports", "logs"] {
            let p = root.join(d);
            fs::create_dir_all(&p).map_err(|e| io(&p, e))?;
        }
        Ok(())
    }

    /// Create a new package and write the initial project.
    pub fn create(root: &Path, project: &Project, media: &[MediaItem]) -> Result<Package> {
        let root = ensure_ext(root);
        if Self::db_path(&root).exists() {
            return Err(StoreError::Io { path: root.display().to_string(), source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "a project already exists here") });
        }
        Self::make_dirs(&root)?;
        let mut conn = Connection::open(Self::db_path(&root))?;
        configure(&conn)?;
        migrate_to(&mut conn, DB_SCHEMA_VERSION)?;
        let mut pkg = Package { root, conn };
        pkg.save(project, media, "create", 0)?;
        pkg.set_setting("session_open", "1")?;
        Ok(pkg)
    }

    /// Open a package, migrate, check integrity and report whether recovery is relevant.
    pub fn open(root: &Path) -> Result<(Package, OpenReport)> {
        let root = root.to_path_buf();
        let db = Self::db_path(&root);
        if !db.exists() {
            return Err(StoreError::NotAProject(root.display().to_string()));
        }
        Self::make_dirs(&root)?;
        let mut conn = Connection::open(&db)?;
        configure(&conn)?;
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).unwrap_or_else(|e| e.to_string());
        if check != "ok" {
            return Err(StoreError::Corrupt(check));
        }
        let before = migrate_to(&mut conn, DB_SCHEMA_VERSION)?;
        let pkg = Package { root, conn };
        let unclean = pkg.setting("session_open")?.as_deref() == Some("1");
        let last_commit_at = pkg.conn.query_row("SELECT at FROM journal ORDER BY seq DESC LIMIT 1", [], |r| r.get::<_, String>(0)).optional()?;
        pkg.set_setting("session_open", "1")?;
        Ok((pkg, OpenReport { unclean_shutdown: unclean, last_commit_at, migrated_from: if before < DB_SCHEMA_VERSION { Some(before) } else { None } }))
    }

    /// Mark a clean close (no recovery prompt next time).
    pub fn close(self) -> Result<()> {
        self.set_setting("session_open", "0")?;
        let _ = self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
        Ok(())
    }

    /// Persist everything in one atomic `BEGIN IMMEDIATE` transaction, then refresh the manifest.
    pub fn save(&mut self, p: &Project, media: &[MediaItem], kind: &str, revision: u64) -> Result<()> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_project(&tx, p)?;
        write_media(&tx, media)?;
        let geom = p.spec.geometry();
        tx.execute("DELETE FROM frames", [])?;
        tx.execute("DELETE FROM texts", [])?;
        tx.execute("DELETE FROM shapes", [])?;
        tx.execute("DELETE FROM spreads", [])?;
        for (i, s) in p.spreads.iter().enumerate() {
            write_spread(&tx, s, i as i64, false, geom.trim_w, geom.trim_h)?;
        }
        if let Some(c) = &p.cover {
            let cg = cover_geometry_for(p);
            write_spread(&tx, &c.content, 0, true, cg.trim_w(), cg.trim_h())?;
            tx.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('cover_meta', ?1)", [serde_json::to_string(&(&c.material_id, &c.stamp))?])?;
        } else {
            tx.execute("DELETE FROM settings WHERE key='cover_meta'", [])?;
        }
        tx.execute("INSERT INTO journal(at, kind, revision) VALUES (?1, ?2, ?3)", params![now_iso(), kind, revision as i64])?;
        tx.execute("DELETE FROM journal WHERE seq < (SELECT MAX(seq) - 500 FROM journal)", [])?;
        tx.commit()?;
        self.write_sidecars(p, media)?;
        Ok(())
    }

    fn write_sidecars(&self, p: &Project, media: &[MediaItem]) -> Result<()> {
        let m = manifest(p, media.len());
        write_atomic(&self.root.join("manifest.json"), serde_json::to_string_pretty(&m)?.as_bytes())?;
        let links: Vec<MediaLink> = media.iter().map(|m| MediaLink { id: m.id.to_string(), path: m.path.clone(), hash: m.hash.clone(), size: m.size }).collect();
        write_atomic(&self.root.join("media").join("links.json"), serde_json::to_string_pretty(&links)?.as_bytes())?;
        Ok(())
    }

    pub fn load(&self) -> Result<(Project, Vec<MediaItem>)> {
        let mut p = read_project(&self.conn)?;
        let media = read_media(&self.conn)?;
        let mut spreads = read_spreads(&self.conn, false)?;
        spreads.sort_by_key(|(i, _)| *i);
        p.spreads = spreads.into_iter().map(|(_, s)| s).collect();
        let cover = read_spreads(&self.conn, true)?.into_iter().next().map(|(_, s)| s);
        if let Some(content) = cover {
            let meta: (Option<String>, Option<Stamp>) = self.setting("cover_meta")?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or((None, None));
            p.cover = Some(Cover { content, material_id: meta.0, stamp: meta.1 });
        }
        let staged = std::mem::take(&mut p.timeline.staged);
        p.timeline = Timeline::from_spreads(&p.spreads);
        p.timeline.staged = staged;
        Ok((p, media))
    }

    /// Point-in-time copy via `VACUUM INTO`. Auto snapshots are rolled (keep last 10).
    pub fn snapshot(&self, reason: &str, manual: bool) -> Result<SnapshotInfo> {
        let id = new_id().to_string();
        let stamp = time::OffsetDateTime::now_utc().format(&time::macros::format_description!("[year][month][day]-[hour][minute][second]")).unwrap_or_default();
        let safe_reason: String = reason.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        // UUIDv7 starts with the timestamp, so use its random tail to keep names unique.
        let tail = &id[id.len().saturating_sub(12)..];
        let path = self.snapshots_dir().join(format!("{stamp}-{safe_reason}-{tail}.sqlite"));
        self.conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
        let created_at = now_iso();
        self.conn.execute("INSERT INTO snapshots(id,path,reason,created_at,manual) VALUES(?1,?2,?3,?4,?5)", params![id, path.to_string_lossy(), reason, created_at, manual as i64])?;
        if !manual {
            let autos: Vec<(String, String)> = {
                let mut st = self.conn.prepare("SELECT id, path FROM snapshots WHERE manual=0 ORDER BY created_at DESC, rowid DESC")?;
                let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.collect::<std::result::Result<_, _>>()?
            };
            for (sid, spath) in autos.into_iter().skip(AUTO_SNAPSHOTS_KEPT) {
                let _ = fs::remove_file(&spath);
                self.conn.execute("DELETE FROM snapshots WHERE id=?1", [sid])?;
            }
        }
        Ok(SnapshotInfo { id, path, reason: reason.into(), created_at, manual })
    }

    pub fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>> {
        let mut st = self.conn.prepare("SELECT id, path, reason, created_at, manual FROM snapshots ORDER BY created_at DESC, rowid DESC")?;
        let rows = st.query_map([], |r| Ok(SnapshotInfo { id: r.get(0)?, path: PathBuf::from(r.get::<_, String>(1)?), reason: r.get(2)?, created_at: r.get(3)?, manual: r.get::<_, i64>(4)? != 0 }))?;
        let mut v: Vec<SnapshotInfo> = rows.collect::<std::result::Result<_, _>>()?;
        v.retain(|s| s.path.exists());
        Ok(v)
    }

    /// Restore a snapshot into the live database (a safety snapshot is taken first).
    pub fn restore_snapshot(&mut self, snap: &Path) -> Result<()> {
        let keep = self.list_snapshots()?;
        self.snapshot("before-restore", true)?;
        let safety = self.list_snapshots()?;
        self.conn.restore(rusqlite::DatabaseName::Main, snap, None::<fn(rusqlite::backup::Progress)>)?;
        migrate_to(&mut self.conn, DB_SCHEMA_VERSION)?;
        // The restored DB has an older snapshots table; re-register the files that exist on disk.
        for s in keep.iter().chain(safety.iter()) {
            self.conn.execute(
                "INSERT OR IGNORE INTO snapshots(id,path,reason,created_at,manual) VALUES(?1,?2,?3,?4,?5)",
                params![s.id, s.path.to_string_lossy(), s.reason, s.created_at, s.manual as i64],
            )?;
        }
        self.set_setting("session_open", "1")?;
        Ok(())
    }

    /// Copy the whole package to a new location (Save As). Cache is copied too (rebuildable).
    pub fn save_as(&mut self, new_root: &Path, p: &Project, media: &[MediaItem]) -> Result<Package> {
        let new_root = ensure_ext(new_root);
        if Self::db_path(&new_root).exists() {
            return Err(StoreError::Io { path: new_root.display().to_string(), source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "a project already exists here") });
        }
        self.save(p, media, "save", 0)?;
        Self::make_dirs(&new_root)?;
        self.conn.execute("VACUUM INTO ?1", [Self::db_path(&new_root).to_string_lossy()])?;
        copy_dir(&self.cache_dir(), &new_root.join("cache"))?;
        let (mut pkg, _) = Package::open(&new_root)?;
        pkg.conn.execute("DELETE FROM snapshots", [])?;
        pkg.save(p, media, "save-as", 0)?;
        Ok(pkg)
    }

    pub fn record_export(&self, kind: &str, path: &Path, preset: &str, audit_json: &str, checksums_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO exports(id,created_at,kind,path,preset,audit_json,checksums_json) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![new_id().to_string(), now_iso(), kind, path.to_string_lossy(), preset, audit_json, checksums_json],
        )?;
        Ok(())
    }

    pub fn export_count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT count(*) FROM exports", [], |r| r.get(0))?)
    }

    pub fn record_proof(&self, path: &Path, note: &str) -> Result<String> {
        let id = new_id().to_string();
        self.conn.execute("INSERT INTO proof_revisions(id,created_at,path,note) VALUES(?1,?2,?3,?4)", params![id, now_iso(), path.to_string_lossy(), note])?;
        Ok(id)
    }

    pub fn save_lab_profile(&self, id: &str, toml: &str) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO lab_profiles(id,toml,added_at) VALUES(?1,?2,?3)", params![id, toml, now_iso()])?;
        Ok(())
    }
    pub fn lab_profile_toml(&self, id: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT toml FROM lab_profiles WHERE id=?1", [id], |r| r.get(0)).optional()?)
    }
    pub fn save_template(&self, id: &str, toml: &str, version: u32) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO templates(id,toml,version) VALUES(?1,?2,?3)", params![id, toml, version])?;
        Ok(())
    }
    pub fn templates(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT toml FROM templates ORDER BY id")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

/// Cover geometry used for storing cover coordinates (falls back to the generic formula).
pub fn cover_geometry_for(p: &Project) -> oto_core::spec::CoverGeometry {
    oto_core::lab::generic_cover_geometry(&p.spec, p.page_count())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|e| io(dst, e))?;
    for e in fs::read_dir(src).map_err(|e| io(src, e))?.flatten() {
        let p = e.path();
        let t = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &t)?;
        } else {
            fs::copy(&p, &t).map_err(|e| io(&p, e))?;
        }
    }
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", new_id().simple()));
    fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io(path, e))
}

pub fn manifest(p: &Project, media_count: usize) -> Manifest {
    let mm = oto_core::units::um_to_mm;
    Manifest {
        schema_version: DB_SCHEMA_VERSION,
        app_version: oto_core::APP_VERSION.into(),
        project_id: p.id.to_string(),
        name: p.name.clone(),
        created_at: p.created_at.clone(),
        modified_at: p.modified_at.clone(),
        spec: ManifestSpec {
            page_width_mm: mm(p.spec.page_width_um),
            page_height_mm: mm(p.spec.page_height_um),
            spread_width_mm: mm(p.spec.spread_width_um),
            bleed_mm: mm(p.spec.bleed_um),
            safe_mm: mm(p.spec.safe_um),
        },
        lab_profile: p.lab_id.clone(),
        spread_count: p.spreads.len(),
        page_count: p.page_count(),
        cover: p.cover.is_some(),
        media_count,
    }
}

pub fn read_manifest(root: &Path) -> Option<Manifest> {
    let s = fs::read_to_string(root.join("manifest.json")).ok()?;
    serde_json::from_str(&s).ok()
}

// ---------------------------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------------------------

fn write_project(tx: &rusqlite::Transaction, p: &Project) -> Result<()> {
    tx.execute("DELETE FROM projects", [])?;
    tx.execute(
        "INSERT INTO projects(id,name,client,created_at,modified_at,due_date,status,lab_id,lab_size_id,settings_json,staged_json,model_schema) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            p.id.to_string(),
            p.name,
            p.client,
            p.created_at,
            p.modified_at,
            p.due_date,
            p.status.key(),
            p.lab_id,
            p.lab_size_id,
            serde_json::to_string(&p.settings)?,
            serde_json::to_string(&p.timeline.staged)?,
            p.schema_version
        ],
    )?;
    let s = &p.spec;
    tx.execute("DELETE FROM album_specs", [])?;
    tx.execute(
        "INSERT INTO album_specs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            p.id.to_string(),
            s.page_width_um,
            s.page_height_um,
            s.spread_width_um,
            s.spread_height_um,
            s.bleed_um,
            s.trim_outer_um,
            s.trim_inside_um,
            s.safe_um,
            s.gutter_um,
            s.min_pages,
            s.max_pages,
            s.page_increment,
            s.units.label(),
            s.cover_type.key(),
            match s.binding {
                Binding::Layflat => "layflat",
                Binding::SinglePages => "single_pages",
            },
            s.target_dpi,
            s.output_profile.name,
            s.output_profile.icc_path
        ],
    )?;
    Ok(())
}

fn read_project(c: &Connection) -> Result<Project> {
    let row = c
        .query_row("SELECT id,name,client,created_at,modified_at,due_date,status,lab_id,lab_size_id,settings_json,staged_json,model_schema FROM projects LIMIT 1", [], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, u32>(11)?,
            ))
        })
        .optional()?
        .ok_or(StoreError::Empty)?;
    let spec = c.query_row("SELECT * FROM album_specs LIMIT 1", [], |r| {
        Ok(AlbumSpec {
            page_width_um: r.get(1)?,
            page_height_um: r.get(2)?,
            spread_width_um: r.get(3)?,
            spread_height_um: r.get(4)?,
            bleed_um: r.get(5)?,
            trim_outer_um: r.get(6)?,
            trim_inside_um: r.get(7)?,
            safe_um: r.get(8)?,
            gutter_um: r.get(9)?,
            min_pages: r.get(10)?,
            max_pages: r.get(11)?,
            page_increment: r.get(12)?,
            units: Units::parse(&r.get::<_, String>(13)?).unwrap_or_default(),
            cover_type: CoverType::parse(&r.get::<_, String>(14)?).unwrap_or_default(),
            binding: if r.get::<_, String>(15)? == "single_pages" { Binding::SinglePages } else { Binding::Layflat },
            target_dpi: r.get(16)?,
            output_profile: ColorProfile { name: r.get(17)?, icc_path: r.get(18)? },
        })
    })?;
    Ok(Project {
        schema_version: row.11,
        id: Uuid::parse_str(&row.0).unwrap_or_else(|_| new_id()),
        name: row.1,
        client: row.2,
        created_at: row.3,
        modified_at: row.4,
        due_date: row.5,
        status: ProjectStatus::parse(&row.6),
        spec,
        lab_id: row.7,
        lab_size_id: row.8,
        spreads: Vec::new(),
        cover: None,
        settings: serde_json::from_str(&row.9).unwrap_or_default(),
        timeline: Timeline { staged: serde_json::from_str(&row.10).unwrap_or_default(), ..Default::default() },
    })
}

fn write_media(tx: &rusqlite::Transaction, media: &[MediaItem]) -> Result<()> {
    tx.execute("DELETE FROM media", [])?;
    let mut st = tx.prepare(
        "INSERT INTO media(id,path,file_name,hash,size,mtime,width,height,orientation,format,capture_time,camera,lens,iso,aperture,shutter,focal_mm,color_profile,rating,color_label,rejected,flagged,client_pick,bins_json,missing,features_json,similar_group,imported_at,sort_index,focus_json)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
    )?;
    for (i, m) in media.iter().enumerate() {
        st.execute(params![
            m.id.to_string(),
            m.path,
            m.file_name,
            m.hash,
            m.size as i64,
            m.mtime,
            m.width,
            m.height,
            m.orientation,
            m.format,
            m.capture_time,
            m.camera,
            m.lens,
            m.iso,
            m.aperture,
            m.shutter,
            m.focal_mm,
            m.color_profile,
            m.rating,
            m.color_label.key(),
            m.rejected,
            m.flagged,
            m.client_pick,
            serde_json::to_string(&m.bins)?,
            m.missing,
            serde_json::to_string(&m.features)?,
            m.similar_group,
            m.imported_at,
            i as i64,
            m.focus.map(|f| serde_json::to_string(&f)).transpose()?,
        ])?;
    }
    Ok(())
}

fn read_media(c: &Connection) -> Result<Vec<MediaItem>> {
    let mut st = c.prepare(
        "SELECT id,path,file_name,hash,size,mtime,width,height,orientation,format,capture_time,camera,lens,iso,aperture,shutter,focal_mm,color_profile,rating,color_label,rejected,flagged,client_pick,bins_json,missing,features_json,similar_group,imported_at,focus_json FROM media ORDER BY sort_index",
    )?;
    let rows = st.query_map([], |r| {
        Ok(MediaItem {
            id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_else(|_| new_id()),
            path: r.get(1)?,
            file_name: r.get(2)?,
            hash: r.get(3)?,
            size: r.get::<_, i64>(4)? as u64,
            mtime: r.get(5)?,
            width: r.get(6)?,
            height: r.get(7)?,
            orientation: r.get(8)?,
            format: r.get(9)?,
            capture_time: r.get(10)?,
            camera: r.get(11)?,
            lens: r.get(12)?,
            iso: r.get(13)?,
            aperture: r.get(14)?,
            shutter: r.get(15)?,
            focal_mm: r.get(16)?,
            color_profile: r.get(17)?,
            rating: r.get(18)?,
            color_label: ColorLabel::parse(&r.get::<_, String>(19)?),
            rejected: r.get(20)?,
            flagged: r.get(21)?,
            client_pick: r.get(22)?,
            bins: serde_json::from_str(&r.get::<_, String>(23)?).unwrap_or_default(),
            missing: r.get(24)?,
            features: serde_json::from_str(&r.get::<_, String>(25)?).unwrap_or_default(),
            similar_group: r.get(26)?,
            imported_at: r.get(27)?,
            focus: r.get::<_, Option<String>>(28)?.and_then(|s| serde_json::from_str(&s).ok()),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn um(r: &oto_core::geometry::Rect, w: Um, h: Um) -> RectUm {
    RectUm::from_norm(r, w, h)
}

fn write_spread(tx: &rusqlite::Transaction, s: &Spread, idx: i64, cover: bool, w: Um, h: Um) -> Result<()> {
    let id = s.id.to_string();
    tx.execute(
        "INSERT INTO spreads(id,idx,is_cover,template_id,background_json,locked,notes,surface_w_um,surface_h_um,explain) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![id, idx, cover, s.template_id, serde_json::to_string(&s.background)?, s.locked, s.notes, w, h, s.explain],
    )?;
    for (i, f) in s.frames.iter().enumerate() {
        let r = um(&f.rect_norm, w, h);
        tx.execute(
            "INSERT INTO frames VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)",
            params![
                f.id.to_string(),
                id,
                i as i64,
                f.z,
                r.x,
                r.y,
                r.w,
                r.h,
                f.image_id.map(|x| x.to_string()),
                f.crop.center_norm.x,
                f.crop.center_norm.y,
                f.crop.zoom,
                f.crop.rotation_mdeg,
                f.crop_manual,
                f.focus.map(|x| serde_json::to_string(&x)).transpose()?,
                f.rotation_mdeg,
                f.opacity,
                serde_json::to_string(&f.border)?,
                f.radius_mm,
                match f.fit {
                    FitMode::Fill => "fill",
                    FitMode::Fit => "fit",
                },
                serde_json::to_string(&f.adjust)?,
                f.locked,
                f.allow_rotate,
                f.role
            ],
        )?;
    }
    for (i, t) in s.texts.iter().enumerate() {
        let r = um(&t.rect_norm, w, h);
        tx.execute(
            "INSERT INTO texts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![t.id.to_string(), id, i as i64, t.z, r.x, r.y, r.w, r.h, t.text, serde_json::to_string(&t.style)?, t.rotation_mdeg, t.locked],
        )?;
    }
    for (i, sh) in s.shapes.iter().enumerate() {
        let r = um(&sh.rect_norm, w, h);
        let style = serde_json::json!({"fill": sh.fill, "stroke": sh.stroke, "stroke_mm": sh.stroke_mm, "opacity": sh.opacity});
        let kind = match sh.kind {
            ShapeKind::Rect => "rect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Line => "line",
        };
        tx.execute(
            "INSERT INTO shapes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![sh.id.to_string(), id, i as i64, sh.z, r.x, r.y, r.w, r.h, kind, style.to_string(), sh.rotation_mdeg, sh.locked],
        )?;
    }
    Ok(())
}

fn uid(s: String) -> Uuid {
    Uuid::parse_str(&s).unwrap_or_else(|_| new_id())
}

fn read_spreads(c: &Connection, cover: bool) -> Result<Vec<(i64, Spread)>> {
    let mut st = c.prepare("SELECT id,idx,template_id,background_json,locked,notes,surface_w_um,surface_h_um,explain FROM spreads WHERE is_cover=?1")?;
    let heads = st.query_map([cover], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, bool>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, String>(8)?,
        ))
    })?;
    let heads: Vec<_> = heads.collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, idx, template_id, bg, locked, notes, w, h, explain) in heads {
        let norm = |x: i64, y: i64, ww: i64, hh: i64| RectUm::new(x, y, ww, hh).to_norm(w, h);
        let mut fs_ = c.prepare("SELECT * FROM frames WHERE spread_id=?1 ORDER BY ord")?;
        let frames = fs_.query_map([&id], |r| {
            let focus: Option<String> = r.get(14)?;
            let border: String = r.get(17)?;
            let adjust: String = r.get(20)?;
            Ok(Frame {
                id: uid(r.get(0)?),
                rect_norm: norm(r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?),
                z: r.get(3)?,
                image_id: r.get::<_, Option<String>>(8)?.map(uid),
                crop: Crop { center_norm: oto_core::geometry::Vec2::new(r.get(9)?, r.get(10)?), zoom: r.get(11)?, rotation_mdeg: r.get(12)? },
                crop_manual: r.get(13)?,
                focus: focus.and_then(|s| serde_json::from_str(&s).ok()),
                rotation_mdeg: r.get(15)?,
                opacity: r.get(16)?,
                border: serde_json::from_str(&border).unwrap_or_default(),
                radius_mm: r.get(18)?,
                fit: if r.get::<_, String>(19)? == "fit" { FitMode::Fit } else { FitMode::Fill },
                adjust: serde_json::from_str(&adjust).unwrap_or_default(),
                locked: r.get(21)?,
                allow_rotate: r.get(22)?,
                role: r.get(23)?,
            })
        })?;
        let frames: Vec<Frame> = frames.collect::<std::result::Result<_, _>>()?;
        let mut ts = c.prepare("SELECT * FROM texts WHERE spread_id=?1 ORDER BY ord")?;
        let texts = ts.query_map([&id], |r| {
            let style: String = r.get(9)?;
            Ok(TextBlock {
                id: uid(r.get(0)?),
                rect_norm: norm(r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?),
                text: r.get(8)?,
                style: serde_json::from_str(&style).unwrap_or_default(),
                rotation_mdeg: r.get(10)?,
                z: r.get(3)?,
                locked: r.get(11)?,
            })
        })?;
        let texts: Vec<TextBlock> = texts.collect::<std::result::Result<_, _>>()?;
        let mut ss = c.prepare("SELECT * FROM shapes WHERE spread_id=?1 ORDER BY ord")?;
        let shapes = ss.query_map([&id], |r| {
            let kind: String = r.get(8)?;
            let style: String = r.get(9)?;
            let v: serde_json::Value = serde_json::from_str(&style).unwrap_or_default();
            let kind = match kind.as_str() {
                "ellipse" => ShapeKind::Ellipse,
                "line" => ShapeKind::Line,
                _ => ShapeKind::Rect,
            };
            let mut s = Shape::new(kind, norm(r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?));
            s.id = uid(r.get(0)?);
            s.z = r.get(3)?;
            s.rotation_mdeg = r.get(10)?;
            s.locked = r.get(11)?;
            s.fill = serde_json::from_value(v["fill"].clone()).unwrap_or(None);
            s.stroke = serde_json::from_value(v["stroke"].clone()).unwrap_or(None);
            s.stroke_mm = v["stroke_mm"].as_f64().unwrap_or(0.5) as f32;
            s.opacity = v["opacity"].as_f64().unwrap_or(1.0) as f32;
            Ok(s)
        })?;
        let shapes: Vec<Shape> = shapes.collect::<std::result::Result<_, _>>()?;
        out.push((idx, Spread { id: uid(id), template_id, background: serde_json::from_str(&bg).unwrap_or_default(), frames, texts, shapes, locked, notes, explain }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oto_core::geometry::Rect;
    use oto_core::media::Features;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oto-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn media(n: usize) -> Vec<MediaItem> {
        (0..n)
            .map(|i| MediaItem {
                id: new_id(),
                path: format!("/photos/IMG_{i}.jpg"),
                file_name: format!("IMG_{i}.jpg"),
                hash: format!("{i:064x}"),
                size: 1000 + i as u64,
                mtime: 1_700_000_000,
                width: 6000,
                height: 4000,
                orientation: 1,
                format: "jpeg".into(),
                capture_time: Some("2026-10-25T10:00:00".into()),
                camera: Some("Test Cam".into()),
                lens: None,
                iso: Some(400),
                aperture: Some(2.8),
                shutter: Some("1/250".into()),
                focal_mm: Some(50.0),
                color_profile: Some("sRGB".into()),
                rating: (i % 6) as u8,
                color_label: ColorLabel::Green,
                rejected: false,
                flagged: i % 2 == 0,
                client_pick: false,
                bins: vec!["Ceremony".into()],
                missing: false,
                features: Features { brightness: 0.5, dhash: 42, ..Default::default() },
                focus: None,
                similar_group: None,
                imported_at: now_iso(),
            })
            .collect()
    }

    fn project(m: &[MediaItem]) -> Project {
        let mut p = Project::new("Aina & Danial", AlbumSpec::default(), 3);
        let mut f = Frame::new(Rect::new(0.1, 0.12, 0.3, 0.5));
        f.image_id = Some(m[0].id);
        f.crop.zoom = 1.25;
        f.crop.rotation_mdeg = 1500;
        f.border.width_mm = 1.5;
        p.spreads[0].frames.push(f);
        p.spreads[0].texts.push(TextBlock::new(Rect::new(0.55, 0.4, 0.4, 0.1), "Title", TextStyle::default()));
        p.spreads[1].shapes.push(Shape::new(ShapeKind::Ellipse, Rect::new(0.6, 0.2, 0.2, 0.2)));
        p.spreads[1].background.pattern = Some(Pattern::new(PatternKind::Dots));
        p.spreads[2].locked = true;
        p.timeline.staged = vec![m[1].id];
        p
    }

    fn approx(a: &Project, b: &Project) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.spec, b.spec);
        assert_eq!(a.spreads.len(), b.spreads.len());
        for (x, y) in a.spreads.iter().zip(&b.spreads) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.locked, y.locked);
            assert_eq!(x.background, y.background);
            assert_eq!(x.frames.len(), y.frames.len());
            for (f, g) in x.frames.iter().zip(&y.frames) {
                assert!((f.rect_norm.x - g.rect_norm.x).abs() < 1e-5);
                assert!((f.rect_norm.w - g.rect_norm.w).abs() < 1e-5);
                assert_eq!(f.image_id, g.image_id);
                assert_eq!(f.crop, g.crop);
                assert_eq!(f.border, g.border);
            }
            assert_eq!(x.texts.iter().map(|t| &t.text).collect::<Vec<_>>(), y.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
            assert_eq!(x.shapes.len(), y.shapes.len());
        }
        assert_eq!(a.timeline.staged, b.timeline.staged);
    }

    #[test]
    fn save_reopen_identical() {
        let d = tmp("roundtrip");
        let m = media(5);
        let p = project(&m);
        let pkg = Package::create(&d.join("Aina"), &p, &m).unwrap();
        assert!(pkg.root.ends_with("Aina.otoalbum"));
        let root = pkg.root.clone();
        pkg.close().unwrap();
        let (pkg, report) = Package::open(&root).unwrap();
        assert!(!report.unclean_shutdown);
        let (p2, m2) = pkg.load().unwrap();
        approx(&p, &p2);
        assert_eq!(m, m2);
        let man = read_manifest(&root).unwrap();
        assert_eq!(man.spread_count, 3);
        assert_eq!(man.spec.page_width_mm, 254.0);
        assert!(root.join("media/links.json").exists());
    }

    #[test]
    fn unclean_shutdown_is_detected() {
        let d = tmp("crash");
        let m = media(2);
        let p = project(&m);
        let pkg = Package::create(&d.join("Crash"), &p, &m).unwrap();
        let root = pkg.root.clone();
        drop(pkg); // simulated crash: no close()
        let (_pkg, report) = Package::open(&root).unwrap();
        assert!(report.unclean_shutdown);
        assert!(report.last_commit_at.is_some());
    }

    #[test]
    fn snapshots_roll_and_restore() {
        let d = tmp("snap");
        let m = media(2);
        let mut p = project(&m);
        let mut pkg = Package::create(&d.join("Snap"), &p, &m).unwrap();
        let first = pkg.snapshot("manual", true).unwrap();
        for _ in 0..12 {
            pkg.snapshot("auto", false).unwrap();
        }
        let list = pkg.list_snapshots().unwrap();
        assert_eq!(list.iter().filter(|s| !s.manual).count(), AUTO_SNAPSHOTS_KEPT);
        p.name = "Changed".into();
        pkg.save(&p, &m, "save", 1).unwrap();
        pkg.restore_snapshot(&first.path).unwrap();
        let (p3, _) = pkg.load().unwrap();
        assert_eq!(p3.name, "Aina & Danial");
        assert!(pkg.list_snapshots().unwrap().iter().any(|s| s.reason == "before-restore"));
    }

    #[test]
    fn migration_v1_to_v2_keeps_data() {
        let d = tmp("migrate");
        let root = d.join("Old.otoalbum");
        Package::make_dirs(&root).unwrap();
        let mut conn = Connection::open(Package::db_path(&root)).unwrap();
        migrate_to(&mut conn, 1).unwrap();
        conn.execute(
            "INSERT INTO spreads(id,idx,is_cover,template_id,background_json,locked,notes,surface_w_um,surface_h_um) VALUES('00000000-0000-7000-8000-000000000001',0,0,'blank',?1,0,'',508000,254000)",
            [serde_json::to_string(&Background::default()).unwrap()],
        )
        .unwrap();
        let p = Project::new("Old", AlbumSpec::default(), 0);
        {
            let tx = conn.transaction().unwrap();
            write_project(&tx, &p).unwrap();
            tx.commit().unwrap();
        }
        drop(conn);
        let (pkg, report) = Package::open(&root).unwrap();
        assert_eq!(report.migrated_from, Some(1));
        let (p2, _) = pkg.load().unwrap();
        assert_eq!(p2.spreads.len(), 1);
        assert_eq!(p2.spreads[0].explain, "");
        // Idempotent.
        let mut c2 = Connection::open(Package::db_path(&root)).unwrap();
        assert_eq!(migrate_to(&mut c2, DB_SCHEMA_VERSION).unwrap(), 2);
    }

    #[test]
    fn corrupt_db_is_reported() {
        let d = tmp("corrupt");
        let root = d.join("Bad.otoalbum");
        fs::create_dir_all(&root).unwrap();
        fs::write(Package::db_path(&root), b"definitely not sqlite at all, sorry").unwrap();
        assert!(Package::open(&root).is_err());
        assert!(matches!(Package::open(&d.join("Nope.otoalbum")), Err(StoreError::NotAProject(_))));
    }

    #[test]
    fn save_as_copies() {
        let d = tmp("saveas");
        let m = media(2);
        let p = project(&m);
        let mut pkg = Package::create(&d.join("A"), &p, &m).unwrap();
        let pkg2 = pkg.save_as(&d.join("B"), &p, &m).unwrap();
        let (p2, _) = pkg2.load().unwrap();
        approx(&p, &p2);
    }
}

#[cfg(test)]
mod sample_tests {
    #[test]
    fn sample_project_opens() {
        let d = std::env::temp_dir().join(format!("oto-sample-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let root = crate::sample::create_sample(&d).unwrap();
        let (pkg, _) = crate::Package::open(&root).unwrap();
        let (p, media) = pkg.load().unwrap();
        assert_eq!(media.len(), 12);
        assert_eq!(p.spreads.len(), 6);
        assert!(p.cover.is_some());
        let used: usize = p.spreads.iter().map(|s| s.media_ids().len()).sum();
        assert_eq!(used, 12);
    }
}
