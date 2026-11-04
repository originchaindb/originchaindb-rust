use crate::Error;
use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) type TokenProvider =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send>> + Send + Sync>;
pub(crate) type Observer = Arc<dyn Fn(RequestEvent) + Send + Sync>;

/// Local transport metadata, excluding URLs, table names, query values, headers,
/// payloads and credentials. HTTP success can precede a result decoding failure.
#[derive(Debug, Clone)]
pub struct RequestEvent {
    pub method: String,
    pub operation: String,
    pub duration: Duration,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    pub error_kind: Option<String>,
}

pub(crate) struct RequestSpan {
    pub event: RequestEvent,
    pub finished: bool,
    started: Instant,
    observer: Option<Observer>,
}
impl RequestSpan {
    pub fn new(method: &str, path: &str, observer: Option<Observer>) -> Self {
        let operation = match path.split('/').next().unwrap_or("") {
            value @ ("sql" | "rows" | "schemas" | "vector" | "fts" | "graph" | "cypher"
            | "usage" | "query-metrics") => value,
            _ => "other",
        };
        Self {
            event: RequestEvent {
                method: method.into(),
                operation: operation.into(),
                duration: Duration::ZERO,
                status: None,
                request_id: None,
                error_kind: None,
            },
            finished: false,
            started: Instant::now(),
            observer,
        }
    }
}
impl Drop for RequestSpan {
    fn drop(&mut self) {
        self.event.duration = self.started.elapsed();
        if !self.finished {
            self.event.error_kind = Some("canceled".into());
        }
        if let Some(observer) = &self.observer {
            let event = self.event.clone();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(event)));
        }
    }
}

/// Conversion failure at a zero-based row index. Display omits row data;
/// std::error::Error::source exposes the native Serde error when requested.
#[derive(Debug)]
pub struct RowDecodeError {
    pub row: usize,
    pub(crate) source: serde_json::Error,
}
impl fmt::Display for RowDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot decode SQL row {}", self.row)
    }
}
impl std::error::Error for RowDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
