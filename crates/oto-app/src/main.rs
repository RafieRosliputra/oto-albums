//! oto-albums desktop app entry point.
#![forbid(unsafe_code)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use std::io::Write;
use std::path::PathBuf;

fn init_logging() -> Option<PathBuf> {
    let dir = oto_platform::app_data_dir().join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("oto-albums.log")).ok()?;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wgpu=warn,naga=warn,eframe=warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_writer(std::sync::Mutex::new(file)).try_init();
    Some(dir)
}

fn install_crash_log(dir: Option<PathBuf>) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = &dir {
            let stamp = time::OffsetDateTime::now_utc().unix_timestamp();
            if let Ok(mut f) = std::fs::File::create(dir.join(format!("crash-{stamp}.log"))) {
                let _ = writeln!(f, "oto-albums {} crashed\n{info}\n\n{}", env!("CARGO_PKG_VERSION"), std::backtrace::Backtrace::force_capture());
            }
        }
        tracing::error!("panic: {info}");
        default(info);
    }));
}

fn main() {
    let logs = init_logging();
    install_crash_log(logs);
    tracing::info!("oto-albums {} starting ({} {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    // A project path may be passed on the command line or by the OS (file association).
    let open = std::env::args().skip(1).map(PathBuf::from).find(|p| p.extension().and_then(|e| e.to_str()) == Some("otoalbum"));
    if let Err(e) = oto_ui::run(open) {
        tracing::error!("fatal: {e}");
        eprintln!("oto-albums could not start: {e}");
        std::process::exit(1);
    }
}
