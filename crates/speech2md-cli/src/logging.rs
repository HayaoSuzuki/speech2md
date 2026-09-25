use std::error::Error;

use tracing::{Subscriber, level_filters::LevelFilter};
use tracing_subscriber::{
    EnvFilter,
    filter::{ParseError, Targets},
    fmt::MakeWriter,
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
};

/// Installs the process-wide stderr logging subscriber.
///
/// # Errors
///
/// Returns an error when `RUST_LOG` is invalid or a global subscriber is already installed.
pub fn init() -> Result<(), Box<dyn Error + Send + Sync>> {
    let directive = match std::env::var("RUST_LOG") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error @ std::env::VarError::NotUnicode(_)) => return Err(error.into()),
    };
    let filter = filter_from(directive.as_deref())?;
    subscriber(filter, std::io::stderr).try_init()?;
    Ok(())
}

fn filter_from(value: Option<&str>) -> Result<EnvFilter, ParseError> {
    EnvFilter::try_new(value.unwrap_or("speech2md=info"))
}

fn subscriber<W>(filter: EnvFilter, writer: W) -> impl Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let project_targets = Targets::new()
        .with_target("speech2md_cli", LevelFilter::TRACE)
        .with_target("speech2md_runtime", LevelFilter::TRACE);
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(writer)
            .with_filter(filter)
            .with_filter(project_targets),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use super::{filter_from, subscriber};

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

    impl Write for CapturedLogs {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("capture log lock").write(buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl CapturedLogs {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("capture log lock").clone())
                .expect("logs are UTF-8")
        }
    }

    #[test]
    fn defaults_to_info_when_rust_log_is_absent() {
        assert_eq!(
            filter_from(None).expect("default filter").to_string(),
            "speech2md=info"
        );
    }

    #[test]
    fn accepts_module_specific_rust_log_filter() {
        assert_eq!(
            filter_from(Some("speech2md_runtime=debug"))
                .expect("valid filter")
                .to_string(),
            "speech2md_runtime=debug"
        );
    }

    #[test]
    fn rejects_an_invalid_rust_log_filter() {
        assert!(filter_from(Some("speech2md_runtime=[invalid")).is_err());
    }

    #[test]
    fn subscriber_applies_filter_and_writes_events() {
        let logs = CapturedLogs::default();
        let writer = logs.clone();
        let collector = subscriber(
            filter_from(Some("debug")).expect("valid filter"),
            move || writer.clone(),
        );

        tracing::subscriber::with_default(collector, || {
            tracing::debug!(target: "speech2md_cli", "visible debug event");
            tracing::info!(target: "speech2md_cli", "visible info event");
            tracing::info!(target: "external_dependency", "https://secret.example/model");
        });

        let output = logs.text();
        assert!(output.contains("visible debug event"));
        assert!(output.contains("visible info event"));
        assert!(!output.contains("secret.example"));
    }
}
