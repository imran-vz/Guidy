//! Recording overlay state machine.
//! Pure: driven by backend events plus a millisecond clock.

use serde_json::Value;

/// Bars shown in the waveform (the first N of 16 FFT buckets).
pub const WAVE_BARS: usize = 9;
const LEVEL_BUCKETS: usize = 16;
/// The Live preview calls out a model load only once it has run this long.
pub const SLOW_MODEL_LOAD_MS: u64 = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind {
    Recording,
    Streaming,
    Transcribing,
    Processing,
}

impl OverlayKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "recording" => Self::Recording,
            "streaming" => Self::Streaming,
            "transcribing" => Self::Transcribing,
            "processing" => Self::Processing,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Listening,
    Working,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkKind {
    Transcribing,
    Polishing,
}

#[derive(Clone, Debug)]
pub struct OverlayModel {
    pub visible: bool,
    pub kind: OverlayKind,
    pub capture_ready: bool,
    ready_at_ms: Option<u64>,
    pub model_loading: bool,
    load_started_ms: Option<u64>,
    pub load_notice_shown: bool,
    smoothed: [f32; LEVEL_BUCKETS],
    pub committed: String,
    pub tentative: String,
    pub phase: Phase,
    pub work_kind: WorkKind,
    /// Bumped per streaming session so the Live card replays its pop-in.
    pub session: u64,
    pub top: bool,
    /// Text auto-follows the newest line while pinned.
    pub pinned: bool,
}

impl Default for OverlayModel {
    fn default() -> Self {
        Self {
            visible: false,
            kind: OverlayKind::Recording,
            capture_ready: false,
            ready_at_ms: None,
            model_loading: false,
            load_started_ms: None,
            load_notice_shown: false,
            smoothed: [0.0; LEVEL_BUCKETS],
            committed: String::new(),
            tentative: String::new(),
            phase: Phase::Listening,
            work_kind: WorkKind::Transcribing,
            session: 0,
            top: false,
            pinned: true,
        }
    }
}

impl OverlayModel {
    /// `overlay_top` is the current `overlay_position == top` setting.
    pub fn apply_event(&mut self, name: &str, payload: &Value, now_ms: u64, overlay_top: bool) {
        match name {
            "show-overlay" => {
                let Some(kind) = payload.as_str().and_then(OverlayKind::parse) else {
                    return;
                };
                // Reset synchronously so a fast recording-ready that follows
                // is never overwritten.
                if matches!(kind, OverlayKind::Recording | OverlayKind::Streaming) {
                    self.capture_ready = false;
                    self.ready_at_ms = None;
                    self.smoothed = [0.0; LEVEL_BUCKETS];
                    self.committed.clear();
                    self.tentative.clear();
                }
                self.top = overlay_top;
                self.kind = kind;
                if kind == OverlayKind::Streaming {
                    self.phase = Phase::Listening;
                    self.work_kind = WorkKind::Transcribing;
                    self.load_notice_shown = false;
                    self.session += 1;
                    self.pinned = true;
                }
                self.visible = true;
                self.refresh_notice(now_ms);
            }
            "hide-overlay" => {
                self.visible = false;
                self.capture_ready = false;
                self.ready_at_ms = None;
            }
            "recording-ready" => {
                self.capture_ready = true;
                self.ready_at_ms = Some(now_ms);
            }
            "mic-level" => {
                let levels: Vec<f32> = serde_json::from_value(payload.clone()).unwrap_or_default();
                for (i, prev) in self.smoothed.iter_mut().enumerate() {
                    let target = levels.get(i).copied().unwrap_or(0.0);
                    *prev = *prev * 0.7 + target * 0.3;
                }
            }
            "stream-text-event" => {
                self.committed = payload["committed"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.tentative = payload["tentative"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
            }
            "stream-phase-event" => {
                self.phase = match payload["phase"].as_str() {
                    Some("working") => Phase::Working,
                    _ => Phase::Listening,
                };
                match payload["kind"].as_str() {
                    Some("polishing") => self.work_kind = WorkKind::Polishing,
                    Some("transcribing") => self.work_kind = WorkKind::Transcribing,
                    _ => {}
                }
            }
            "model-state-changed" => match payload["event_type"].as_str() {
                // Only completed/failed end a load; other events (e.g. the idle
                // watcher's `unloaded`) aren't ordered against it.
                Some("loading_started") => {
                    self.model_loading = true;
                    self.load_started_ms = Some(now_ms);
                }
                Some("loading_completed") | Some("loading_failed") => {
                    self.model_loading = false;
                    self.load_started_ms = None;
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Advance clock-driven state (slow-load latch).
    pub fn tick(&mut self, now_ms: u64) {
        self.refresh_notice(now_ms);
    }

    fn refresh_notice(&mut self, now_ms: u64) {
        if self.model_load_slow(now_ms) && self.kind == OverlayKind::Streaming {
            self.load_notice_shown = true;
        }
    }

    pub fn model_load_slow(&self, now_ms: u64) -> bool {
        self.load_started_ms
            .is_some_and(|start| now_ms.saturating_sub(start) >= SLOW_MODEL_LOAD_MS)
    }

    pub fn levels(&self) -> [f32; WAVE_BARS] {
        let mut out = [0.0; WAVE_BARS];
        out.copy_from_slice(&self.smoothed[..WAVE_BARS]);
        out
    }

    /// Whole seconds since capture became ready (counts only while streaming
    /// and visible).
    pub fn elapsed_secs(&self, now_ms: u64) -> u64 {
        if self.kind != OverlayKind::Streaming || !self.visible {
            return 0;
        }
        self.ready_at_ms
            .map(|at| now_ms.saturating_sub(at) / 1000)
            .unwrap_or(0)
    }

    pub fn has_text(&self) -> bool {
        !self.committed.is_empty() || !self.tentative.is_empty()
    }

    pub fn live_layout(&self, now_ms: u64) -> LiveLayout {
        let working = self.phase == Phase::Working;
        let has_text = self.has_text();
        let loading_notice = self.model_load_slow(now_ms) && !working && !has_text;
        let open = has_text || loading_notice || (self.load_notice_shown && !working);
        LiveLayout {
            working,
            loading_notice,
            open,
            collapsed: working && !has_text,
            show_caret: !working && !loading_notice,
            show_timer: open,
        }
    }

    /// i18n key for the working label.
    pub fn working_label_key(&self) -> &'static str {
        let transcribing = if self.model_loading {
            "overlay.loadingModel"
        } else {
            "overlay.transcribing"
        };
        match self.kind {
            OverlayKind::Streaming => {
                if self.work_kind == WorkKind::Polishing {
                    "overlay.processing"
                } else {
                    transcribing
                }
            }
            OverlayKind::Processing => "overlay.processing",
            _ => transcribing,
        }
    }

    /// Compact form shows the working row for transcribing/processing.
    pub fn compact_working(&self) -> bool {
        matches!(
            self.kind,
            OverlayKind::Transcribing | OverlayKind::Processing
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveLayout {
    pub working: bool,
    pub loading_notice: bool,
    pub open: bool,
    pub collapsed: bool,
    pub show_caret: bool,
    pub show_timer: bool,
}

/// Bar height in px: clamp(3, 3 + v^0.7 · 15, 18).
pub fn bar_height(v: f32) -> f32 {
    (3.0 + v.max(0.0).powf(0.7) * 15.0).clamp(3.0, 18.0)
}

/// `m:ss`
pub fn format_timer(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Re-pin when within 16 px of the bottom.
pub fn is_pinned(scroll_height: f32, scroll_top: f32, client_height: f32) -> bool {
    scroll_height - scroll_top - client_height <= 16.0
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use serde_json::json;

    fn show(m: &mut OverlayModel, kind: &str, now: u64) {
        m.apply_event("show-overlay", &json!(kind), now, false);
    }

    #[test]
    fn show_resets_capture_and_text_synchronously() {
        let mut m = OverlayModel::default();
        m.capture_ready = true;
        m.committed = "old".into();
        show(&mut m, "recording", 0);
        assert!(m.visible && !m.capture_ready && m.committed.is_empty());
        m.apply_event("recording-ready", &Value::Null, 10, false);
        assert!(m.capture_ready);
        // Transcribing keeps the text (no reset)
        m.committed = "keep".into();
        show(&mut m, "transcribing", 20);
        assert_eq!(m.committed, "keep");
        m.apply_event("hide-overlay", &Value::Null, 30, false);
        assert!(!m.visible && !m.capture_ready);
    }

    #[test]
    fn streaming_session_bumps_and_resets_phase() {
        let mut m = OverlayModel::default();
        m.phase = Phase::Working;
        m.work_kind = WorkKind::Polishing;
        show(&mut m, "streaming", 0);
        assert_eq!(m.session, 1);
        assert_eq!(m.phase, Phase::Listening);
        assert_eq!(m.work_kind, WorkKind::Transcribing);
        m.apply_event("show-overlay", &json!("streaming"), 0, true);
        assert_eq!(m.session, 2);
        assert!(m.top);
    }

    #[test]
    fn levels_are_smoothed_and_truncated() {
        let mut m = OverlayModel::default();
        m.apply_event("mic-level", &json!(vec![1.0f32; 16]), 0, false);
        assert!((m.levels()[0] - 0.3).abs() < 1e-6);
        m.apply_event("mic-level", &json!(vec![1.0f32; 16]), 0, false);
        assert!((m.levels()[0] - 0.51).abs() < 1e-6);
        assert_eq!(m.levels().len(), WAVE_BARS);
    }

    #[test]
    fn bar_heights_and_timer() {
        assert_eq!(bar_height(0.0), 3.0);
        assert_eq!(bar_height(1.0), 18.0);
        assert_eq!(bar_height(5.0), 18.0);
        assert!((bar_height(0.5) - (3.0 + 0.5f32.powf(0.7) * 15.0)).abs() < 1e-5);
        assert_eq!(format_timer(0), "0:00");
        assert_eq!(format_timer(65), "1:05");
        assert!(is_pinned(200.0, 120.0, 64.0));
        assert!(!is_pinned(200.0, 50.0, 64.0));
    }

    #[test]
    fn slow_load_notice_only_after_2s_and_only_cleared_by_completion() {
        let mut m = OverlayModel::default();
        show(&mut m, "streaming", 0);
        m.apply_event(
            "model-state-changed",
            &json!({"event_type": "loading_started"}),
            0,
            false,
        );
        assert!(m.model_loading);
        assert!(!m.live_layout(1999).loading_notice);
        m.tick(2000);
        let l = m.live_layout(2000);
        assert!(l.loading_notice && l.open && !l.show_caret);
        assert!(m.load_notice_shown);
        // `unloaded` must not clear an in-flight load
        m.apply_event(
            "model-state-changed",
            &json!({"event_type": "unloaded"}),
            2100,
            false,
        );
        assert!(m.model_loading);
        m.apply_event(
            "model-state-changed",
            &json!({"event_type": "loading_completed"}),
            2200,
            false,
        );
        assert!(!m.model_loading);
        // Latched notice keeps the panel open while listening.
        assert!(m.live_layout(2300).open);
    }

    #[test]
    fn live_layout_states() {
        let mut m = OverlayModel::default();
        show(&mut m, "streaming", 0);
        let l = m.live_layout(0);
        assert!(!l.open && !l.collapsed && l.show_caret);
        m.apply_event(
            "stream-text-event",
            &json!({"committed": "hello", "tentative": "wor"}),
            0,
            false,
        );
        assert!(m.live_layout(0).open);
        m.apply_event(
            "stream-phase-event",
            &json!({"phase": "working", "kind": "polishing"}),
            0,
            false,
        );
        let l = m.live_layout(0);
        assert!(l.working && l.open && !l.collapsed && !l.show_caret);
        assert_eq!(m.working_label_key(), "overlay.processing");
        m.committed.clear();
        m.tentative.clear();
        assert!(m.live_layout(0).collapsed);
    }

    #[test]
    fn working_labels() {
        let mut m = OverlayModel::default();
        show(&mut m, "transcribing", 0);
        assert!(m.compact_working());
        assert_eq!(m.working_label_key(), "overlay.transcribing");
        m.model_loading = true;
        assert_eq!(m.working_label_key(), "overlay.loadingModel");
        show(&mut m, "processing", 0);
        assert_eq!(m.working_label_key(), "overlay.processing");
    }

    #[test]
    fn timer_counts_from_capture_ready_while_streaming() {
        let mut m = OverlayModel::default();
        show(&mut m, "streaming", 0);
        assert_eq!(m.elapsed_secs(5000), 0, "not ready yet");
        m.apply_event("recording-ready", &Value::Null, 1000, false);
        assert_eq!(m.elapsed_secs(3500), 2);
        show(&mut m, "recording", 4000);
        assert_eq!(m.elapsed_secs(9000), 0, "compact form has no timer");
    }
}
