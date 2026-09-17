//! Structured local logs: JSON lines, daily rotation, categories via tracing targets.
//! Nothing here logs tokens or pixel data.

use std::path::Path;

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init(log_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all(log_dir).ok()?;
    let file_appender = tracing_appender::rolling::daily(log_dir, "mimic.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_env("MIMIC_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,hyper=warn,tower_http=warn,tao=warn,wry=warn"));
    let file_layer = fmt::layer().json().with_target(true).with_writer(non_blocking);
    let stderr_layer =
        if cfg!(debug_assertions) { Some(fmt::layer().compact().with_writer(std::io::stderr)) } else { None };
    tracing_subscriber::registry().with(filter).with(file_layer).with(stderr_layer).try_init().ok();
    prune_old_logs(log_dir, 14);
    Some(guard)
}

fn prune_old_logs(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("mimic.log"))
        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, e.path())))
        .collect();
    files.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}
