//! An exporter that keeps everything in memory, for tests.

use std::sync::{Arc, Mutex, PoisonError};

use crate::export::Exporter;
use crate::trace::{LogRecord, SpanRecord};

/// Keeps every span and log.
#[derive(Debug, Clone, Default)]
pub struct MemoryExporter {
    spans: Arc<Mutex<Vec<SpanRecord>>>,
    logs: Arc<Mutex<Vec<LogRecord>>>,
}

impl MemoryExporter {
    /// The spans so far.
    #[must_use]
    pub fn spans(&self) -> Vec<SpanRecord> {
        self.spans.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The logs so far.
    #[must_use]
    pub fn logs(&self) -> Vec<LogRecord> {
        self.logs.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Exporter for MemoryExporter {
    fn span(&self, _: &str, span: &SpanRecord) {
        self.spans.lock().unwrap_or_else(PoisonError::into_inner).push(span.clone());
    }
    fn log(&self, _: &str, log: &LogRecord) {
        self.logs.lock().unwrap_or_else(PoisonError::into_inner).push(log.clone());
    }
}
