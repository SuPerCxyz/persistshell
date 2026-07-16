use std::collections::VecDeque;
use std::mem::size_of;

use persist_ipc::{Completeness, TrendPoint, TrendScope, MAX_TREND_POINTS};

use super::aggregate::{aggregate, TrendSeries};
use super::model::{DerivedSample, DerivedSession};

pub(super) const MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
pub(super) const HISTORY_WINDOW_MS: u64 = 60 * 60 * 1_000;
pub(super) const HISTORY_FRAME_LIMIT: usize = 720;

pub(super) struct BoundedHistory {
    frames: VecDeque<Box<DerivedSample>>,
    memory_bytes: usize,
    base_bytes: usize,
    max_bytes: usize,
    max_age_ms: u64,
    max_frames: usize,
}

impl BoundedHistory {
    pub(super) fn new() -> Self {
        Self::with_limits(MEMORY_LIMIT_BYTES, HISTORY_WINDOW_MS, HISTORY_FRAME_LIMIT)
    }

    pub(super) fn with_limits(max_bytes: usize, max_age_ms: u64, max_frames: usize) -> Self {
        let max_frames = max_frames.max(1);
        let frames = VecDeque::with_capacity(max_frames);
        let base_bytes = size_of::<Self>() + frames.capacity() * size_of::<Box<DerivedSample>>();
        assert!(max_bytes >= base_bytes + size_of::<DerivedSample>());
        Self {
            frames,
            memory_bytes: base_bytes,
            base_bytes,
            max_bytes,
            max_age_ms,
            max_frames,
        }
    }

    pub(super) fn push(&mut self, mut sample: DerivedSample) {
        self.limit_sample(&mut sample);
        let sample_bytes = sample_bytes(&sample);
        while self.frames.len() >= self.max_frames
            || self.memory_bytes.saturating_add(sample_bytes) > self.max_bytes
        {
            if !self.pop_front() {
                break;
            }
        }
        let newest = sample.monotonic_ms;
        self.memory_bytes += sample_bytes;
        self.frames.push_back(Box::new(sample));
        while self
            .frames
            .front()
            .is_some_and(|oldest| newest.saturating_sub(oldest.monotonic_ms) > self.max_age_ms)
        {
            self.pop_front();
        }
    }

    pub(super) fn len(&self) -> usize {
        self.frames.len()
    }

    pub(super) fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub(super) fn latest(&self) -> Option<&DerivedSample> {
        self.frames.back().map(Box::as_ref)
    }

    pub(super) fn oldest_monotonic_ms(&self) -> Option<u64> {
        self.frames.front().map(|sample| sample.monotonic_ms)
    }

    pub(super) fn trend(
        &self,
        scope: TrendScope,
        now_ms: u64,
        window_ms: u64,
        max_points: u16,
    ) -> TrendSeries {
        if window_ms == 0 || max_points == 0 || max_points > MAX_TREND_POINTS {
            return TrendSeries::unavailable();
        }
        let start_ms = now_ms.saturating_sub(window_ms);
        aggregate(&self.frames, scope, start_ms, now_ms, max_points)
    }

    pub(super) fn minute(&self, scope: TrendScope, start_ms: u64) -> Option<TrendPoint> {
        let end_ms = start_ms.saturating_add(60_000);
        aggregate(&self.frames, scope, start_ms, end_ms, 1)
            .points
            .into_iter()
            .next()
    }

    fn limit_sample(&self, sample: &mut DerivedSample) {
        sample
            .sessions
            .sort_unstable_by_key(|session| session.metrics.session_id);
        let available = self
            .max_bytes
            .saturating_sub(self.base_bytes)
            .saturating_sub(size_of::<DerivedSample>());
        let max_sessions = available / size_of::<DerivedSession>();
        if sample.sessions.len() > max_sessions {
            sample.sessions.truncate(max_sessions);
            sample.completeness = Completeness::Partial;
        }
        sample.sessions = std::mem::take(&mut sample.sessions)
            .into_boxed_slice()
            .into_vec();
    }

    fn pop_front(&mut self) -> bool {
        let Some(sample) = self.frames.pop_front() else {
            return false;
        };
        self.memory_bytes -= sample_bytes(&sample);
        true
    }
}

fn sample_bytes(sample: &DerivedSample) -> usize {
    size_of::<DerivedSample>() + sample.sessions.capacity() * size_of::<DerivedSession>()
}
