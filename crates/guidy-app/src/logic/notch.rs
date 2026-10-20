//! The meeting controls around the notch: what they show and where they go.
//! Pure: driven by recorder states, polled input levels and a clock.

use guidy_core::meetings::types::{format_elapsed, RecorderState};
use std::collections::VecDeque;

/// Width of each wing beside the notch, in points.
pub const WING: f64 = 84.0;
/// Gap left in the middle on displays without a notch.
pub const NO_NOTCH_GAP: f64 = 12.0;
/// Height used when the display reports no menu-bar inset.
pub const FALLBACK_HEIGHT: f64 = 32.0;
/// Bars in the waveform (newest on the right).
pub const BARS: usize = 11;

/// A display, in Cocoa coordinates (points, origin bottom-left of the
/// primary display).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenInfo {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Horizontal extent of the camera housing, relative to the screen's
    /// left edge; `None` on displays without a notch.
    pub notch: Option<(f64, f64)>,
    /// Height of the area above the safe area (the notch / menu bar).
    pub top_inset: f64,
}

/// Where the controls go, in Cocoa coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotchFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Width of the middle part the camera housing covers.
    pub notch_width: f64,
}

/// Wings on both sides of the notch, flush with the top edge. Without a
/// notch, a small island in the middle of the menu bar.
pub fn notch_frame(s: &ScreenInfo) -> NotchFrame {
    let height = if s.top_inset > 0.0 {
        s.top_inset
    } else {
        FALLBACK_HEIGHT
    };
    let (center, notch_width) = match s.notch {
        Some((x0, x1)) if x1 > x0 => ((x0 + x1) / 2.0, x1 - x0),
        _ => (s.width / 2.0, NO_NOTCH_GAP),
    };
    let width = notch_width + 2.0 * WING;
    NotchFrame {
        x: (s.x + center - width / 2.0).round(),
        y: s.y + s.height - height,
        width,
        height,
        notch_width,
    }
}

#[derive(Clone, Debug)]
pub struct NotchModel {
    pub recorder: RecorderState,
    levels: VecDeque<f32>,
}

impl Default for NotchModel {
    fn default() -> Self {
        Self {
            recorder: RecorderState::Idle,
            levels: VecDeque::from(vec![0.0; BARS]),
        }
    }
}

impl NotchModel {
    pub fn visible(&self) -> bool {
        self.recorder.is_recording()
    }

    pub fn paused(&self) -> bool {
        self.recorder.is_paused()
    }

    /// A new recording starts with a flat waveform.
    pub fn set_recorder(&mut self, state: RecorderState) {
        let started = match (&self.recorder, &state) {
            (
                RecorderState::Recording { meeting_id: a, .. },
                RecorderState::Recording { meeting_id: b, .. },
            ) => a != b,
            (RecorderState::Idle, RecorderState::Recording { .. }) => true,
            _ => false,
        };
        if started {
            self.levels.iter_mut().for_each(|l| *l = 0.0);
        }
        self.recorder = state;
    }

    /// Scroll in one level sample (flat while paused).
    pub fn push_level(&mut self, level: f32) {
        let level = if self.paused() {
            0.0
        } else {
            level.clamp(0.0, 1.0)
        };
        self.levels.pop_front();
        self.levels.push_back(level);
    }

    pub fn levels(&self) -> impl Iterator<Item = f32> + '_ {
        self.levels.iter().copied()
    }

    /// Capture time so far (pauses excluded), e.g. `12:34`.
    pub fn timer(&self, now_ms: i64) -> String {
        format_elapsed(self.recorder.elapsed_ms(now_ms).unwrap_or(0))
    }
}

/// Bar height in px for a level: 3 px at silence up to `max`.
pub fn bar_height(level: f32, max: f32) -> f32 {
    (3.0 + level.max(0.0).powf(0.6) * (max - 3.0)).clamp(3.0, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording(id: i64, paused_at: Option<i64>) -> RecorderState {
        RecorderState::Recording {
            meeting_id: id,
            title: "Sync".into(),
            started_at: 0,
            scheduled_end_at: None,
            paused_at,
            paused_ms: 0,
        }
    }

    #[test]
    fn wings_flank_the_notch() {
        let s = ScreenInfo {
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 982.0,
            notch: Some((662.0, 850.0)),
            top_inset: 38.0,
        };
        let f = notch_frame(&s);
        assert_eq!(f.notch_width, 188.0);
        assert_eq!(f.width, 188.0 + 2.0 * WING);
        assert_eq!(f.x, 662.0 - WING);
        assert_eq!((f.y, f.height), (982.0 - 38.0, 38.0));
    }

    #[test]
    fn without_a_notch_it_sits_mid_menu_bar_on_that_screen() {
        let s = ScreenInfo {
            x: 1512.0,
            y: -300.0,
            width: 2560.0,
            height: 1440.0,
            notch: None,
            top_inset: 0.0,
        };
        let f = notch_frame(&s);
        assert_eq!(f.height, FALLBACK_HEIGHT);
        assert_eq!(f.y, -300.0 + 1440.0 - FALLBACK_HEIGHT);
        assert_eq!(f.x + f.width / 2.0, 1512.0 + 1280.0);
    }

    #[test]
    fn levels_scroll_and_flatten_while_paused() {
        let mut m = NotchModel::default();
        assert!(!m.visible());
        m.set_recorder(recording(1, None));
        assert!(m.visible());
        m.push_level(0.8);
        assert_eq!(m.levels().last(), Some(0.8));
        assert_eq!(m.levels().count(), BARS);
        m.set_recorder(recording(1, Some(10)));
        m.push_level(0.8);
        assert_eq!(m.levels().last(), Some(0.0));
        // Same meeting: history kept. A new one starts flat.
        assert!(m.levels().any(|l| l > 0.0));
        m.set_recorder(recording(2, None));
        assert!(m.levels().all(|l| l == 0.0));
    }

    #[test]
    fn timer_and_bars() {
        let mut m = NotchModel::default();
        m.set_recorder(recording(1, Some(65_000)));
        assert_eq!(m.timer(500_000), "1:05");
        assert_eq!(bar_height(0.0, 16.0), 3.0);
        assert_eq!(bar_height(1.0, 16.0), 16.0);
    }
}
