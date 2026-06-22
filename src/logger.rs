use anyhow::Context;
use serde::Serialize;
use std::time::Instant;

pub fn init_logger() -> anyhow::Result<()> {
    let log_file = std::env::var("LOG_FILE").unwrap_or_else(|_| "./agent.log".to_string());
    let log_level_str = std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    let log_level = match log_level_str.to_lowercase().as_str() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Debug,
    };

    log::info!("[LOGGER] log level: {}, log file: {}", log_level, log_file);

    let module_filters = [
        ("surrealdb", log::LevelFilter::Warn),
        ("surreal", log::LevelFilter::Warn),
        ("async_openai", log::LevelFilter::Warn),
        ("rmcp", log::LevelFilter::Warn),
        ("hyper", log::LevelFilter::Warn),
        ("h2", log::LevelFilter::Warn),
        ("tower", log::LevelFilter::Warn),
        ("rustls", log::LevelFilter::Warn),
        ("tokio_rustls", log::LevelFilter::Warn),
        ("reqwest", log::LevelFilter::Warn),
    ];

    let file_dispatch = {
        let mut d = fern::Dispatch::new()
            .format(|out, message, record| {
                out.finish(format_args!(
                    "[{}][{}] {}",
                    record.level(),
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    message
                ))
            })
            .level(log_level);
        for (module, level) in &module_filters {
            d = d.level_for(*module, *level);
        }
        d = d.level_for("nail_agent", log_level);
        d.chain(fern::log_file(&log_file).context("failed to open log file")?)
    };

    let stdout_dispatch = {
        let mut d = fern::Dispatch::new()
            .format(|out, message, record| {
                out.finish(format_args!(
                    "[{}][{}] {}",
                    record.level(),
                    chrono::Local::now().format("%H:%M:%S%.3f"),
                    message
                ))
            })
            .level(log_level);
        for (module, level) in &module_filters {
            d = d.level_for(*module, *level);
        }
        d = d.level_for("nail_agent", log_level);
        d.chain(std::io::stdout())
    };

    fern::Dispatch::new()
        .chain(file_dispatch)
        .chain(stdout_dispatch)
        .apply()
        .context("failed to apply log config")?;

    Ok(())
}

pub fn log_json<T: Serialize>(label: &str, value: &T) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let json_str =
        serde_json::to_string(value).unwrap_or_else(|e| format!("serialization failed: {}", e));
    log::debug!(
        "=== {} (JSON) ===\n{}\n{}",
        label,
        json_str,
        "=".repeat(label.len() + 12)
    );
}

#[derive(Debug)]
pub struct Timer {
    label: String,
    start: Instant,
}

impl Timer {
    pub fn start(label: impl Into<String>) -> Self {
        let label: String = label.into();
        let start = Instant::now();
        log::debug!("[TIMER] {} started", label);
        Self { label, start }
    }

    pub fn stop(&self) -> u128 {
        let elapsed = self.start.elapsed().as_millis();
        log::info!("[TIMER] {} finished, elapsed: {} ms", self.label, elapsed);
        elapsed
    }

    pub fn lap(&self, note: &str) {
        let elapsed = self.start.elapsed().as_millis();
        log::debug!("[TIMER] {} | {} ({} ms elapsed)", self.label, note, elapsed);
    }
}

#[macro_export]
macro_rules! log_duration {
    ($label:expr, $duration:expr) => {
        log::info!("[DURATION] {} elapsed: {} ms", $label, $duration);
    };
    ($label:expr, $duration:expr, $($extra:tt)*) => {
        log::info!("[DURATION] {} elapsed: {} ms | {}", $label, $duration, format_args!($($extra)*));
    };
}
