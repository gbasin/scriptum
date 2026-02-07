// Event debouncer for the file watcher pipeline.
//
// Coalesces rapid filesystem events on the same path within a configurable
// time window (default 100ms, range 50–500ms). The last event kind wins.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{FsEventKind, RawFsEvent};

/// Default debounce window.
const DEFAULT_DEBOUNCE_MS: u64 = 100;
/// Minimum allowed debounce window.
const MIN_DEBOUNCE_MS: u64 = 50;
/// Maximum allowed debounce window.
const MAX_DEBOUNCE_MS: u64 = 500;

/// Configuration for the debouncer.
#[derive(Debug, Clone)]
pub struct DebounceConfig {
    pub window: Duration,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self { window: Duration::from_millis(DEFAULT_DEBOUNCE_MS) }
    }
}

impl DebounceConfig {
    /// Create a config with the given window in milliseconds, clamped to [50, 500].
    pub fn with_millis(ms: u64) -> Self {
        let clamped = ms.clamp(MIN_DEBOUNCE_MS, MAX_DEBOUNCE_MS);
        Self { window: Duration::from_millis(clamped) }
    }
}

/// Tracks pending events per path with their timestamps.
struct PendingEvent {
    kind: FsEventKind,
    last_seen: Instant,
}

/// Debounces raw filesystem events, coalescing rapid events on the same path.
///
/// Call `push()` for each incoming event, then `drain_ready()` periodically
/// to collect events whose debounce window has elapsed.
pub struct Debouncer {
    config: DebounceConfig,
    pending: HashMap<PathBuf, PendingEvent>,
}

impl Debouncer {
    pub fn new(config: DebounceConfig) -> Self {
        Self { config, pending: HashMap::new() }
    }

    /// Record a new raw filesystem event. If there's already a pending event
    /// for this path, it gets coalesced (last event kind wins, timer resets).
    pub fn push(&mut self, event: RawFsEvent) {
        self.push_at(event, Instant::now());
    }

    /// Like `push` but with a specific timestamp (for testing).
    fn push_at(&mut self, event: RawFsEvent, now: Instant) {
        self.pending.insert(event.path, PendingEvent { kind: event.kind, last_seen: now });
    }

    /// Drain all events whose debounce window has elapsed.
    /// Returns the coalesced events ready for processing.
    pub fn drain_ready(&mut self) -> Vec<RawFsEvent> {
        self.drain_ready_at(Instant::now())
    }

    /// Like `drain_ready` but with a specific timestamp (for testing).
    fn drain_ready_at(&mut self, now: Instant) -> Vec<RawFsEvent> {
        let window = self.config.window;
        let mut ready = Vec::new();

        self.pending.retain(|path, pending| {
            if now.duration_since(pending.last_seen) >= window {
                ready.push(RawFsEvent { kind: pending.kind.clone(), path: path.clone() });
                false // remove from pending
            } else {
                true // keep pending
            }
        });

        ready
    }

    /// Number of events still waiting in the debounce window.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Time until the next pending event becomes ready, or None if empty.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|p| p.last_seen + self.config.window).min()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::*;

    fn event(kind: FsEventKind, path: &str) -> RawFsEvent {
        RawFsEvent { kind, path: PathBuf::from(path) }
    }

    // ── DebounceConfig ─────────────────────────────────────────────

    #[test]
    fn default_config_is_100ms() {
        let config = DebounceConfig::default();
        assert_eq!(config.window, Duration::from_millis(100));
    }

    #[test]
    fn config_clamps_below_minimum() {
        let config = DebounceConfig::with_millis(10);
        assert_eq!(config.window, Duration::from_millis(50));
    }

    #[test]
    fn config_clamps_above_maximum() {
        let config = DebounceConfig::with_millis(1000);
        assert_eq!(config.window, Duration::from_millis(500));
    }

    #[test]
    fn config_accepts_valid_range() {
        let config = DebounceConfig::with_millis(200);
        assert_eq!(config.window, Duration::from_millis(200));
    }

    // ── Single event lifecycle ─────────────────────────────────────

    #[test]
    fn single_event_not_ready_before_window() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now);

        // 50ms later — still within 100ms window.
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(50));
        assert!(ready.is_empty());
        assert_eq!(debouncer.pending_count(), 1);
    }

    #[test]
    fn single_event_ready_after_window() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now);

        // 100ms later — window elapsed.
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(100));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].path, PathBuf::from("/a.md"));
        assert_eq!(ready[0].kind, FsEventKind::Modify);
        assert_eq!(debouncer.pending_count(), 0);
    }

    // ── Coalescing rapid events ────────────────────────────────────

    #[test]
    fn rapid_events_coalesce_last_kind_wins() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        // Three events on the same path within the window.
        debouncer.push_at(event(FsEventKind::Create, "/a.md"), now);
        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now + Duration::from_millis(20));
        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now + Duration::from_millis(40));

        // Only 1 pending event (coalesced).
        assert_eq!(debouncer.pending_count(), 1);

        // Not ready at 80ms (40ms since last event, window is 100ms).
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(80));
        assert!(ready.is_empty());

        // Ready at 140ms (100ms since last event at 40ms).
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(140));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].kind, FsEventKind::Modify); // last kind wins
    }

    #[test]
    fn coalesce_resets_timer() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now);

        // 80ms later, another event on the same path resets the timer.
        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now + Duration::from_millis(80));

        // At 100ms since original, NOT ready (only 20ms since last).
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(100));
        assert!(ready.is_empty());

        // At 180ms (100ms since the 80ms event).
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(180));
        assert_eq!(ready.len(), 1);
    }

    // ── Multiple paths independently ───────────────────────────────

    #[test]
    fn different_paths_tracked_independently() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now);
        debouncer.push_at(event(FsEventKind::Create, "/b.md"), now + Duration::from_millis(50));

        assert_eq!(debouncer.pending_count(), 2);

        // At 100ms: /a.md is ready (100ms since t=0), /b.md not (50ms since t=50).
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(100));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].path, PathBuf::from("/a.md"));
        assert_eq!(debouncer.pending_count(), 1);

        // At 150ms: /b.md is now ready.
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(150));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].path, PathBuf::from("/b.md"));
        assert_eq!(debouncer.pending_count(), 0);
    }

    // ── Remove event coalescing ────────────────────────────────────

    #[test]
    fn create_then_remove_coalesces_to_remove() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Create, "/a.md"), now);
        debouncer.push_at(event(FsEventKind::Remove, "/a.md"), now + Duration::from_millis(30));

        let ready = debouncer.drain_ready_at(now + Duration::from_millis(130));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].kind, FsEventKind::Remove);
    }

    // ── Drain idempotency ──────────────────────────────────────────

    #[test]
    fn drain_ready_is_idempotent() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now);

        let ready = debouncer.drain_ready_at(now + Duration::from_millis(100));
        assert_eq!(ready.len(), 1);

        // Second drain should be empty.
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(200));
        assert!(ready.is_empty());
    }

    // ── Empty debouncer ────────────────────────────────────────────

    #[test]
    fn drain_empty_returns_empty() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let ready = debouncer.drain_ready();
        assert!(ready.is_empty());
        assert_eq!(debouncer.pending_count(), 0);
    }

    // ── next_deadline ──────────────────────────────────────────────

    #[test]
    fn next_deadline_none_when_empty() {
        let debouncer = Debouncer::new(DebounceConfig::default());
        assert!(debouncer.next_deadline().is_none());
    }

    #[test]
    fn next_deadline_returns_earliest() {
        let mut debouncer = Debouncer::new(DebounceConfig::default());
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now);
        debouncer.push_at(event(FsEventKind::Create, "/b.md"), now + Duration::from_millis(50));

        let deadline = debouncer.next_deadline().unwrap();
        // Earliest event was at `now`, so deadline should be now + 100ms.
        assert_eq!(deadline, now + Duration::from_millis(100));
    }

    // ── Custom window ──────────────────────────────────────────────

    #[test]
    fn custom_window_respected() {
        let mut debouncer = Debouncer::new(DebounceConfig::with_millis(200));
        let now = Instant::now();

        debouncer.push_at(event(FsEventKind::Modify, "/a.md"), now);

        // Not ready at 150ms.
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(150));
        assert!(ready.is_empty());

        // Ready at 200ms.
        let ready = debouncer.drain_ready_at(now + Duration::from_millis(200));
        assert_eq!(ready.len(), 1);
    }
}
