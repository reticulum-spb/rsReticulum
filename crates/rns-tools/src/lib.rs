//! Shared utility code for Reticulum CLI tools.

pub mod format;
pub mod hash;

/// rsReticulum package version printed by CLI `--version` output.
pub const RS_RETICULUM_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Python Reticulum version these tools track for CLI/protocol parity.
pub const RETICULUM_COMPAT_VERSION: &str = "1.5.2";

/// Python 1.3.2 `[logging] logtimestamps` (Reticulum.py:459-461,
/// RNS/__init__.py:85 default True): whether log lines carry a timestamp
/// prefix. Read pre-init from the config file, like the loglevel.
pub fn config_log_timestamps(config_dir: &std::path::Path) -> bool {
    rns_runtime::config::Config::from_file(config_dir.join(rns_runtime::config::CONFIG_FILE_NAME))
        .ok()
        .map(|config| config.logging.timestamps)
        .unwrap_or(true)
}

/// Read the optional target filter before initializing the CLI logger.
pub fn config_log_filter(config_dir: &std::path::Path) -> Option<String> {
    rns_runtime::config::Config::from_file(config_dir.join(rns_runtime::config::CONFIG_FILE_NAME))
        .ok()
        .and_then(|config| config.logging.filter)
}

fn logging_targets(
    level: tracing::Level,
    environment: Option<&str>,
    config: Option<&str>,
) -> Result<tracing_subscriber::filter::Targets, String> {
    let (source, text) = match (environment, config) {
        (Some(text), _) => ("RUST_LOG", text),
        (_, Some(text)) => ("logging.filter", text),
        _ => return Ok(tracing_subscriber::filter::Targets::new().with_default(level)),
    };
    // Empty settings mean no overrides, never a target with an empty prefix.
    if text.trim().is_empty() {
        return Ok(tracing_subscriber::filter::Targets::new().with_default(level));
    }
    let mut targets = text
        .parse::<tracing_subscriber::filter::Targets>()
        .map_err(|error| format!("invalid {source}: {error}"))?;
    if targets.default_level().is_none() {
        targets = targets.with_default(level);
    }
    Ok(targets)
}

/// Shared tracing setup; honors RUST_LOG while preserving existing callers.
pub fn init_tracing<W>(level: tracing::Level, timestamps: bool, ansi: bool, writer: W)
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    init_tracing_with_filter(level, timestamps, ansi, writer, None);
}

/// Apply the target filter globally, including the optional web log layer.
pub fn init_tracing_with_filter<W>(
    level: tracing::Level,
    timestamps: bool,
    ansi: bool,
    writer: W,
    config_filter: Option<&str>,
) where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    use tracing_subscriber::prelude::*;

    let environment = match std::env::var("RUST_LOG") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => {
            eprintln!("invalid RUST_LOG: {error}");
            std::process::exit(2);
        }
    };
    let targets =
        logging_targets(level, environment.as_deref(), config_filter).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2);
        });
    let subscriber = tracing_subscriber::registry().with(targets);
    #[cfg(feature = "api")]
    let subscriber = subscriber.with(rns_runtime::web_logs::WebLogLayer);
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(ansi)
        .with_writer(writer);
    if timestamps {
        let _ = subscriber.with(layer).try_init();
    } else {
        let _ = subscriber.with(layer.without_time()).try_init();
    }
}

#[cfg(test)]
mod logging_tests {
    use super::*;
    use tracing::Level;

    #[test]
    fn filter_priority_defaults_and_longest_prefix() {
        let filter = logging_targets(
            Level::WARN,
            None,
            Some("info,rns_interface=debug,rns_interface::plugin=trace,rns_transport=off"),
        )
        .unwrap();
        assert!(filter.would_enable("rns_interface::plugin::child", &Level::TRACE));
        assert!(!filter.would_enable("rns_interface::tcp", &Level::TRACE));
        assert!(filter.would_enable("rns_interface::tcp", &Level::DEBUG));
        assert!(filter.would_enable("other", &Level::INFO));
        assert!(!filter.would_enable("rns_transport::storage", &Level::ERROR));
        let env = logging_targets(Level::WARN, Some("error"), Some("trace")).unwrap();
        assert!(!env.would_enable("rns_interface::plugin", &Level::WARN));
        let partial = logging_targets(
            Level::INFO,
            Some("rns_interface::plugin=debug"),
            Some("trace"),
        )
        .unwrap();
        assert!(partial.would_enable("other", &Level::INFO));
        assert!(!partial.would_enable("other", &Level::DEBUG));
        let empty = logging_targets(Level::WARN, Some(""), Some("trace")).unwrap();
        assert!(!empty.would_enable("other", &Level::INFO));
        assert!(
            logging_targets(Level::INFO, None, Some("plugin=bogus"))
                .unwrap_err()
                .contains("logging.filter")
        );
        assert!(
            logging_targets(Level::INFO, Some("plugin=bogus"), Some("info"))
                .unwrap_err()
                .contains("RUST_LOG")
        );
    }

    #[test]
    fn global_filter_applies_to_all_layers() {
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::{layer::Context, prelude::*};
        #[derive(Clone)]
        struct Capture(Arc<Mutex<Vec<String>>>);
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Capture {
            fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
                self.0
                    .lock()
                    .unwrap()
                    .push(event.metadata().target().to_owned());
            }
        }
        let first = Capture(Arc::default());
        let second = Capture(Arc::default());
        let subscriber = tracing_subscriber::registry()
            .with(
                logging_targets(
                    Level::INFO,
                    None,
                    Some("info,rns_interface::plugin=debug,rns_transport=off"),
                )
                .unwrap(),
            )
            .with(first.clone())
            .with(second.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "rns_interface::plugin", "included");
            tracing::debug!(target: "other", "excluded");
            tracing::error!(target: "rns_transport", "excluded");
            tracing::info!(target: "other", "included");
        });
        assert_eq!(
            *first.0.lock().unwrap(),
            vec!["rns_interface::plugin", "other"]
        );
        assert_eq!(*first.0.lock().unwrap(), *second.0.lock().unwrap());
    }
}
