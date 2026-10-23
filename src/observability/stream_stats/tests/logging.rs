use std::{
    fmt::{self, Write as _},
    sync::{Arc, Mutex},
};

use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer, Registry};

use crate::observability::stream_stats::persistence::log_stream_storage_failure;
use crate::storage::StorageError;

#[derive(Clone)]
struct EventCapture {
    events: Arc<Mutex<Vec<String>>>,
}

impl<S> Layer<S> for EventCapture
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = FieldVisitor(String::new());
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("lock captured tracing events")
            .push(format!("{} {}", event.metadata().name(), visitor.0));
    }
}

struct FieldVisitor(String);

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let _ = write!(self.0, "{}={value:?};", field.name());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let _ = write!(self.0, "{}={value};", field.name());
    }
}

#[test]
fn stream_log_write_failure_excludes_error_details() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Registry::default().with(EventCapture {
        events: Arc::clone(&events),
    });
    let error = StorageError::ValidationFailed(
        "storage error: SELECT provider_key WHERE device_credential = 'device-secret'".to_string(),
    );

    tracing::subscriber::with_default(subscriber, || {
        log_stream_storage_failure("insert", &error);
    });

    let events = events.lock().expect("lock captured tracing events");
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("operation=insert"));
    assert!(events[0].contains("failure_kind=stream_log_storage"));
    assert!(!events[0].contains("provider_key"));
    assert!(!events[0].contains("device_credential"));
    assert!(!events[0].contains("device-secret"));
}
