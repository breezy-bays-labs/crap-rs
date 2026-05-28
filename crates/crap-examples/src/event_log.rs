//! Baseline anchor: low complexity, high coverage → CRAP near 1.
//!
//! Append-only event log with structured serde records. Each function
//! does one straight-line thing: no branching, no nested conditionals.
//! Cognitive complexity stays at 1-2 across the surface, and every
//! branch is exercised by the tests below, so the CRAP score lands in
//! the Low band. This module exists to anchor the bottom of the
//! pedagogical heatmap — without it readers have no Low-band reference
//! point to compare the higher-CRAP modules against.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub kind: String,
    pub message: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventLog {
    pub events: Vec<Event>,
}

impl EventLog {
    pub fn new() -> Self {
        EventLog { events: Vec::new() }
    }

    pub fn append(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn last(&self) -> Option<&Event> {
        self.events.last()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}

pub fn make_event(kind: &str, message: &str, timestamp_ms: u64) -> Event {
    Event {
        kind: kind.to_string(),
        message: message.to_string(),
        timestamp_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_log_is_empty() {
        let log = EventLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert!(log.last().is_none());
    }

    #[test]
    fn append_increases_length() {
        let mut log = EventLog::new();
        log.append(make_event("start", "boot", 0));
        log.append(make_event("step", "init", 100));
        assert_eq!(log.len(), 2);
        assert!(!log.is_empty());
    }

    #[test]
    fn last_returns_most_recent() {
        let mut log = EventLog::new();
        log.append(make_event("a", "first", 1));
        log.append(make_event("b", "second", 2));
        let last = log.last().expect("log has events");
        assert_eq!(last.kind, "b");
        assert_eq!(last.message, "second");
    }

    #[test]
    fn clear_resets_log() {
        let mut log = EventLog::new();
        log.append(make_event("x", "y", 0));
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn make_event_populates_fields() {
        let event = make_event("kind", "message", 42);
        assert_eq!(event.kind, "kind");
        assert_eq!(event.message, "message");
        assert_eq!(event.timestamp_ms, 42);
    }

    #[test]
    fn default_event_log_is_empty() {
        let log = EventLog::default();
        assert!(log.is_empty());
    }
}
