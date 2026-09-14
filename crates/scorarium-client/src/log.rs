use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use http::StatusCode;
use url::Url;

const CAPACITY: usize = 200;

/// One attempt at fulfilling a request.
#[derive(Clone, Debug)]
pub struct CallRecord {
    /// The source's display name
    pub source: &'static str,
    pub url: Url,
    pub attempt: u8, // 1-based
    pub outcome: Outcome,
    pub duration: Duration,
    pub at: SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Status(StatusCode),
    Timeout,
    /// A transport failure: DNS, connection, TLS.
    Error(String),
}

/// How the sources have been doing
#[derive(Clone, Debug)]
pub struct CallHistory {
    /// Newest first, oldest dropped once the history is at capacity
    pub calls: VecDeque<CallRecord>,
    pub cache_hits: u64,
}

/// The most recent attempts across every source, oldest dropped.
///
/// Cache hits are counted rather than recorded individually.
#[derive(Default)]
pub(crate) struct CallLog {
    records: Mutex<VecDeque<CallRecord>>,
    cache_hits: AtomicU64,
}

impl CallLog {
    pub(crate) fn record(&self, record: CallRecord) {
        let mut records = self.records.lock().unwrap();
        if records.len() == CAPACITY {
            records.pop_front();
        }
        records.push_back(record);
    }

    pub(crate) fn cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn history(&self) -> CallHistory {
        CallHistory {
            calls: self.records.lock().unwrap().iter().rev().cloned().collect(),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
        }
    }
}
