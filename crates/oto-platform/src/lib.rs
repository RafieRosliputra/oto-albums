//! oto-platform: OS integration behind one trait. `cfg` lives here, never in the core crates.
//!
//! * Dialogs: `rfd` → `IFileDialog` on Windows, `NSOpenPanel`/`NSSavePanel` on macOS.
//! * Reveal: Explorer `/select,` or Finder `open -R`.
//! * File association: macOS via the app bundle's Info.plist (`.otoalbum` as a document package);
//!   Windows via per-user `HKCU\Software\Classes` (no admin).
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("{0}")]
    Failed(String),
    #[error("not supported on this platform yet: {0}")]
    Unsupported(&'static str),
}

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;
    fn pick_files(&self, title: &str) -> Option<Vec<PathBuf>>;
    fn pick_folder(&self, title: &str) -> Option<PathBuf>;
    fn pick_project(&self) -> Option<PathBuf>;
    fn save_project(&self, default_name: &str) -> Option<PathBuf>;
    fn pick_file(&self, title: &str, ext: &[&str]) -> Option<PathBuf>;
    fn reveal(&self, path: &Path) -> Result<(), PlatformError>;
    fn open(&self, path: &Path) -> Result<(), PlatformError>;
    /// Name of the display's ICC profile, when the OS exposes it cheaply.
    fn display_profile(&self) -> Option<String>;
    fn register_file_association(&self, exe: &Path) -> Result<String, PlatformError>;
    fn documents_dir(&self) -> PathBuf;
}

pub struct Native;

const IMAGE_EXT: [&str; 8] = ["jpg", "jpeg", "jpe", "png", "tif", "tiff", "psd", "webp"];

fn run(cmd: &mut Command) -> Result<(), PlatformError> {
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(PlatformError::Failed(format!("{cmd:?} exited with {s}"))),
        Err(e) => Err(PlatformError::Failed(format!("{cmd:?}: {e}"))),
    }
}

impl Platform for Native {
    fn name(&self) -> &'static str {
        std::env::consts::OS
    }

    fn pick_files(&self, title: &str) -> Option<Vec<PathBuf>> {
        rfd::FileDialog::new().set_title(title).add_filter("Images", &IMAGE_EXT).pick_files()
    }

    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    fn pick_project(&self) -> Option<PathBuf> {
        // A project is a folder; on macOS with the bundled Info.plist it behaves like a document.
        let p = rfd::FileDialog::new()
            .set_title("Open oto-albums project")
            .add_filter("oto-albums project", &["otoalbum"])
            .pick_file()
            .or_else(|| rfd::FileDialog::new().set_title("Open oto-albums project (folder)").pick_folder())?;
        // If the user picked a file inside the package, use the package.
        let mut cur = Some(p.as_path());
        while let Some(c) = cur {
            if c.extension().and_then(|e| e.to_str()) == Some("otoalbum") {
                return Some(c.to_path_buf());
            }
            cur = c.parent();
        }
        Some(p)
    }

    fn save_project(&self, default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("Save oto-albums project").set_directory(self.documents_dir()).set_file_name(format!("{default_name}.otoalbum")).save_file()
    }

    fn pick_file(&self, title: &str, ext: &[&str]) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).add_filter(ext.join(", "), ext).pick_file()
    }

    fn reveal(&self, path: &Path) -> Result<(), PlatformError> {
        if cfg!(target_os = "macos") {
            run(Command::new("open").arg("-R").arg(path))
        } else if cfg!(target_os = "windows") {
            // explorer returns 1 even on success; ignore the status.
            let mut arg = std::ffi::OsString::from("/select,");
            arg.push(path.as_os_str());
            Command::new("explorer").arg(arg).spawn().map(|_| ()).map_err(|e| PlatformError::Failed(e.to_string()))
        } else {
            run(Command::new("xdg-open").arg(path.parent().unwrap_or(path)))
        }
    }

    fn open(&self, path: &Path) -> Result<(), PlatformError> {
        if cfg!(target_os = "macos") {
            run(Command::new("open").arg(path))
        } else if cfg!(target_os = "windows") {
            Command::new("cmd").args(["/C", "start", ""]).arg(path).spawn().map(|_| ()).map_err(|e| PlatformError::Failed(e.to_string()))
        } else {
            run(Command::new("xdg-open").arg(path))
        }
    }

    fn display_profile(&self) -> Option<String> {
        // Reading the display profile needs ColorSync / WCS calls; deferred (see STATUS.md).
        None
    }

    fn register_file_association(&self, exe: &Path) -> Result<String, PlatformError> {
        if cfg!(target_os = "macos") {
            Ok("Handled by the app bundle (Info.plist declares .otoalbum).".into())
        } else if cfg!(target_os = "windows") {
            let exe = exe.display().to_string();
            let cmds: [Vec<String>; 3] = [
                vec!["add".into(), r"HKCU\Software\Classes\.otoalbum".into(), "/ve".into(), "/d".into(), "otoalbums.project".into(), "/f".into()],
                vec!["add".into(), r"HKCU\Software\Classes\otoalbums.project".into(), "/ve".into(), "/d".into(), "oto-albums project".into(), "/f".into()],
                vec!["add".into(), r"HKCU\Software\Classes\otoalbums.project\shell\open\command".into(), "/ve".into(), "/d".into(), format!("\"{exe}\" \"%1\""), "/f".into()],
            ];
            for c in cmds {
                run(Command::new("reg").args(c))?;
            }
            Ok("Registered .otoalbum for the current user.".into())
        } else {
            Err(PlatformError::Unsupported("file association"))
        }
    }

    fn documents_dir(&self) -> PathBuf {
        directories::UserDirs::new()
            .and_then(|u| u.document_dir().map(Path::to_path_buf))
            .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

pub fn native() -> Box<dyn Platform> {
    Box::new(Native)
}

/// Per-user app data folder (settings, recent list, crash logs, user lab profiles).
pub fn app_data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "JohorStudio", "oto-albums").map(|p| p.data_dir().to_path_buf()).unwrap_or_else(|| std::env::temp_dir().join("oto-albums"))
}

/// macOS Quick Look for one file (like pressing Space in Finder). Returns the child process so the
/// caller can close the preview again; `None` on other systems or when it cannot start.
pub fn quick_look(path: &std::path::Path) -> Option<std::process::Child> {
    if !cfg!(target_os = "macos") || !path.exists() {
        return None;
    }
    std::process::Command::new("qlmanage").arg("-p").arg(path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().ok()
}
