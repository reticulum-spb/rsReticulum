use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

const LOG_CAPACITY: usize = 1000;

#[derive(Clone, Debug, Serialize)]
pub struct WebLogEntry {
    pub id: u64,
    pub timestamp_ms: u128,
    pub level: String,
    pub target: String,
    pub message: String,
}

struct WebLogHub {
    next_id: AtomicU64,
    history: RwLock<VecDeque<WebLogEntry>>,
    sender: broadcast::Sender<WebLogEntry>,
}

fn hub() -> &'static WebLogHub {
    static HUB: OnceLock<WebLogHub> = OnceLock::new();
    HUB.get_or_init(|| {
        let (sender, _) = broadcast::channel(1024);
        WebLogHub {
            next_id: AtomicU64::new(1),
            history: RwLock::new(VecDeque::with_capacity(LOG_CAPACITY)),
            sender,
        }
    })
}

pub fn history() -> Vec<WebLogEntry> {
    hub().history.read().unwrap().iter().cloned().collect()
}

pub fn subscribe() -> broadcast::Receiver<WebLogEntry> {
    hub().sender.subscribe()
}

#[derive(Clone, Copy)]
pub struct WebLogLayer;

impl<S: Subscriber> Layer<S> for WebLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let entry = WebLogEntry {
            id: hub().next_id.fetch_add(1, Ordering::Relaxed),
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            level: metadata.level().as_str().to_ascii_lowercase(),
            target: metadata.target().to_string(),
            message: visitor.finish(),
        };
        let mut history = hub().history.write().unwrap();
        if history.len() == LOG_CAPACITY {
            history.pop_front();
        }
        history.push_back(entry.clone());
        drop(history);
        let _ = hub().sender.send(entry);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl MessageVisitor {
    fn push(&mut self, field: &tracing::field::Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            let sensitive = ["password", "passphrase", "token", "secret", "rpc_key"]
                .iter()
                .any(|needle| field.name().to_ascii_lowercase().contains(needle));
            self.fields.push(format!(
                "{}={}",
                field.name(),
                if sensitive { "[redacted]" } else { &value }
            ));
        }
    }

    fn finish(self) -> String {
        let mut parts = Vec::new();
        if let Some(message) = self.message {
            parts.push(message);
        }
        parts.extend(self.fields);
        parts.join(" ")
    }
}

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.push(field, value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.push(field, format!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn layer_captures_events_and_redacts_sensitive_fields() {
        let subscriber = tracing_subscriber::registry().with(WebLogLayer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                target: "web_log_test",
                password = "do-not-expose",
                port = 8080,
                "API started"
            );
        });
        let entry = history().last().cloned().unwrap();
        assert_eq!(entry.target, "web_log_test");
        assert!(entry.message.contains("API started"));
        assert!(entry.message.contains("password=[redacted]"));
        assert!(entry.message.contains("port=8080"));
        assert!(!entry.message.contains("do-not-expose"));
    }
}
