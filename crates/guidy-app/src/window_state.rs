//! The main window's frame and section, saved across launches in
//! `window-state.json` in the app data directory.
//!
//! The frame is recorded as the window moves and resizes and written when
//! the window closes or the app quits. Without a shell (tests, snapshots)
//! nothing is read or written.

use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FILE_NAME: &str = "window-state.json";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    /// Windowed frame in global display points: x, y, width, height.
    #[serde(default)]
    pub frame: Option<[f32; 4]>,
    /// The last selected sidebar section (`Section::id`).
    #[serde(default)]
    pub section: Option<String>,
}

impl SavedWindow {
    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        let [x, y, w, h] = self.frame?;
        Some(Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        })
    }
}

/// The latest windowed frame of the open main window.
pub struct MainFrame(pub Bounds<Pixels>);
impl Global for MainFrame {}

fn path(cx: &App) -> Option<PathBuf> {
    let g = cx.try_global::<crate::windows::ShellGlobal>()?;
    guidy_core::portable::app_data_dir(&g.app)
        .ok()
        .map(|d| d.join(FILE_NAME))
}

/// The saved state, if any (missing or unreadable files give `None`).
pub fn load(cx: &App) -> Option<SavedWindow> {
    let text = std::fs::read_to_string(path(cx)?).ok()?;
    serde_json::from_str(&text).ok()
}

/// Record the main window's frame (call on every bounds change).
pub fn record(window: &Window, cx: &mut App) {
    // Maximized and fullscreen windows keep their windowed frame.
    cx.set_global(MainFrame(window.window_bounds().get_bounds()));
}

/// Write the last recorded frame and the current section.
pub fn save(cx: &App) {
    let Some(path) = path(cx) else {
        return;
    };
    let mut state = load(cx).unwrap_or_default();
    if let Some(MainFrame(b)) = cx.try_global::<MainFrame>() {
        state.frame = Some([
            f32::from(b.origin.x),
            f32::from(b.origin.y),
            f32::from(b.size.width),
            f32::from(b.size.height),
        ]);
    }
    if let Some(s) = cx.try_global::<crate::ui::main_window::LastSection>() {
        state.section = Some(s.0.id().to_string());
    }
    match serde_json::to_string(&state) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("Failed to save the window state: {e}");
            }
        }
        Err(e) => log::warn!("Failed to encode the window state: {e}"),
    }
}

/// `saved` if it is at least `min` and still overlaps a connected display.
pub fn usable_bounds(
    saved: Bounds<Pixels>,
    min: Size<Pixels>,
    displays: &[Bounds<Pixels>],
) -> Option<Bounds<Pixels>> {
    if saved.size.width < min.width || saved.size.height < min.height {
        return None;
    }
    // At least a title bar's worth must be on some display.
    let visible = displays.iter().any(|d| {
        let i = d.intersect(&saved);
        i.size.width >= px(100.) && i.size.height >= px(30.)
    });
    visible.then_some(saved)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui_kit's `test` macro would shadow `#[test]`.
    use super::{usable_bounds, SavedWindow};
    use gpui_kit::{point, px, size, Bounds, Pixels};

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    #[test]
    fn saved_frames_must_fit_the_minimum_and_a_display() {
        let min = size(px(720.), px(480.));
        let displays = [b(0., 0., 1440., 900.)];
        let ok = b(100., 100., 900., 600.);
        assert_eq!(usable_bounds(ok, min, &displays), Some(ok));
        assert_eq!(
            usable_bounds(b(100., 100., 500., 600.), min, &displays),
            None
        );
        // On a display that is no longer connected.
        assert_eq!(
            usable_bounds(b(3000., 100., 900., 600.), min, &displays),
            None
        );
    }

    #[test]
    fn state_round_trips_and_tolerates_missing_fields() {
        let s = SavedWindow {
            frame: Some([1., 2., 900., 600.]),
            section: Some("history".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<SavedWindow>(&json).unwrap(), s);
        assert_eq!(
            serde_json::from_str::<SavedWindow>("{}").unwrap(),
            SavedWindow::default()
        );
    }
}
