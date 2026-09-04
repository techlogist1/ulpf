//! Bounded ring of the most recently emitted JSON lines, read by the server. The output
//! thread hands over each batch's buffer once (moved, not copied) and the ring keeps byte
//! ranges into it; the oldest lines are evicted when the ring is full. A reader that fell
//! behind learns how many lines it missed instead of stalling the engine.

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::{Arc, Mutex};

pub struct Tail {
    inner: Mutex<Inner>,
    capacity: usize,
}

struct Inner {
    entries: VecDeque<Entry>,
}

struct Entry {
    raw_id: u64,
    buf: Arc<Vec<u8>>,
    range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TailFrame {
    /// `(raw id, JSON line without its terminator)`, oldest first.
    pub events: Vec<(u64, Vec<u8>)>,
    /// Lines newer than the caller's position that were evicted or cut by `limit`.
    pub skipped: u64,
    pub latest: Option<u64>,
}

impl Tail {
    pub fn new(capacity: usize) -> Tail {
        Tail { inner: Mutex::new(Inner { entries: VecDeque::with_capacity(capacity.min(4096)) }), capacity: capacity.max(1) }
    }

    /// One emitted batch: `count` lines starting at `first_raw_id`, one per `\n`.
    pub fn push_batch(&self, first_raw_id: u64, buf: Vec<u8>) {
        let buf = Arc::new(buf);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut start = 0;
        let mut id = first_raw_id;
        for (i, b) in buf.iter().enumerate() {
            if *b == b'\n' {
                inner.entries.push_back(Entry { raw_id: id, buf: Arc::clone(&buf), range: start..i });
                start = i + 1;
                id += 1;
            }
        }
        while inner.entries.len() > self.capacity {
            inner.entries.pop_front();
        }
    }

    /// Lines newer than `after` (all of them when `None`), the newest `limit` of them.
    pub fn since(&self, after: Option<u64>, limit: usize) -> TailFrame {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let latest = inner.entries.back().map(|e| e.raw_id);
        let newer: Vec<&Entry> = match after {
            Some(a) => inner.entries.iter().filter(|e| e.raw_id > a).collect(),
            None => inner.entries.iter().collect(),
        };
        let mut skipped = 0u64;
        if let (Some(a), Some(oldest)) = (after, inner.entries.front().map(|e| e.raw_id))
            && oldest > a + 1
        {
            skipped += oldest - (a + 1);
        }
        let cut = newer.len().saturating_sub(limit);
        skipped += cut as u64;
        let events = newer[cut..].iter().map(|e| (e.raw_id, e.buf[e.range.clone()].to_vec())).collect();
        TailFrame { events, skipped, latest }
    }

    pub fn find(&self, raw_id: u64) -> Option<Vec<u8>> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let first = inner.entries.front()?.raw_id;
        if raw_id < first {
            return None;
        }
        // ids are contiguous in emission order, so the position is arithmetic; verify anyway
        let idx = (raw_id - first) as usize;
        inner.entries.get(idx).filter(|e| e.raw_id == raw_id).map(|e| e.buf[e.range.clone()].to_vec())
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn latest(&self) -> Option<u64> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).entries.back().map(|e| e.raw_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_evicts_oldest_and_reports_what_a_slow_reader_missed() {
        let tail = Tail::new(5);
        tail.push_batch(0, b"a\nb\nc\n".to_vec());
        assert_eq!(tail.since(None, 10).events.len(), 3);
        tail.push_batch(3, b"d\ne\nf\ng\n".to_vec());
        let f = tail.since(None, 10);
        assert_eq!(f.events.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![2, 3, 4, 5, 6]);
        assert_eq!(f.latest, Some(6));
        // a reader at id 0 missed id 1 (evicted) and gets the rest
        let f = tail.since(Some(0), 10);
        assert_eq!(f.skipped, 1);
        assert_eq!(f.events.first().map(|(id, _)| *id), Some(2));
        // limit cuts the oldest and counts them
        let f = tail.since(Some(2), 2);
        assert_eq!(f.events.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![5, 6]);
        assert_eq!(f.skipped, 2);
        assert_eq!(tail.find(4), Some(b"e".to_vec()));
        assert_eq!(tail.find(1), None);
        assert_eq!(tail.find(99), None);
        assert_eq!(tail.since(Some(6), 10).events.len(), 0);
    }
}
