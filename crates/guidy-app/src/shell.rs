//! The GPUI implementation of the backend's [`Shell`]: window operations the
//! backend triggers from any thread are marshalled onto GPUI's main thread.

use guidy_core::host::{Monitor, OverlayPlacement, Shell, Theme as HostTheme};
use parking_lot::Mutex;
use std::thread::ThreadId;

pub enum ShellMsg {
    Run(Box<dyn FnOnce() + Send>),
    ShowMain,
    HideMain,
    SetTheme(Option<HostTheme>),
    ShowOverlay(OverlayPlacement),
    MoveOverlay(OverlayPlacement),
    HideOverlay,
    Accessory(bool),
    Exit(i32),
}

/// Facts the backend queries synchronously; refreshed by the main loop.
#[derive(Default)]
pub struct ShellCache {
    pub monitors: Vec<Monitor>,
    pub overlay_size: Option<(f64, f64)>,
    pub system_dark: Option<bool>,
    pub main_visible: bool,
}

pub struct GpuiShell {
    main_thread: ThreadId,
    tx: smol::channel::Sender<ShellMsg>,
    pub cache: Mutex<ShellCache>,
}

impl GpuiShell {
    /// Must be created on the main thread.
    pub fn new(tx: smol::channel::Sender<ShellMsg>) -> Self {
        Self {
            main_thread: std::thread::current().id(),
            tx,
            cache: Mutex::new(ShellCache::default()),
        }
    }

    fn send(&self, msg: ShellMsg) {
        if self.tx.try_send(msg).is_err() {
            log::warn!("UI loop is gone; dropping shell message");
        }
    }

    pub fn is_main_thread(&self) -> bool {
        std::thread::current().id() == self.main_thread
    }
}

impl Shell for GpuiShell {
    fn run_on_main_thread(&self, f: Box<dyn FnOnce() + Send>) {
        if self.is_main_thread() {
            f();
        } else {
            self.send(ShellMsg::Run(f));
        }
    }

    fn show_main_window(&self) {
        self.send(ShellMsg::ShowMain);
    }

    fn hide_main_window(&self) {
        self.send(ShellMsg::HideMain);
    }

    fn is_main_window_visible(&self) -> bool {
        self.cache.lock().main_visible
    }

    fn set_theme(&self, theme: Option<HostTheme>) {
        self.send(ShellMsg::SetTheme(theme));
    }

    fn system_theme(&self) -> Option<HostTheme> {
        self.cache.lock().system_dark.map(|dark| {
            if dark {
                HostTheme::Dark
            } else {
                HostTheme::Light
            }
        })
    }

    fn monitors(&self) -> Vec<Monitor> {
        self.cache.lock().monitors.clone()
    }

    fn show_overlay(&self, placement: OverlayPlacement) {
        self.cache.lock().overlay_size = Some((placement.width, placement.height));
        self.send(ShellMsg::ShowOverlay(placement));
    }

    fn move_overlay(&self, placement: OverlayPlacement) {
        self.send(ShellMsg::MoveOverlay(placement));
    }

    fn hide_overlay(&self) {
        self.cache.lock().overlay_size = None;
        self.send(ShellMsg::HideOverlay);
    }

    fn overlay_size(&self) -> Option<(f64, f64)> {
        self.cache.lock().overlay_size
    }

    fn set_activation_policy_accessory(&self, accessory: bool) {
        self.send(ShellMsg::Accessory(accessory));
    }

    fn exit(&self, code: i32) {
        self.send(ShellMsg::Exit(code));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_thread_work_runs_inline_and_other_threads_queue() {
        let (tx, rx) = smol::channel::unbounded();
        let shell = std::sync::Arc::new(GpuiShell::new(tx));
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let r = ran.clone();
        shell.run_on_main_thread(Box::new(move || {
            r.store(true, std::sync::atomic::Ordering::SeqCst)
        }));
        assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
        assert!(rx.is_empty());

        let s = shell.clone();
        std::thread::spawn(move || s.run_on_main_thread(Box::new(|| {})))
            .join()
            .unwrap();
        assert!(matches!(rx.try_recv(), Ok(ShellMsg::Run(_))));
    }

    #[test]
    fn overlay_size_tracks_show_and_hide() {
        let (tx, _rx) = smol::channel::unbounded();
        let shell = GpuiShell::new(tx);
        shell.show_overlay(OverlayPlacement {
            state: "streaming".into(),
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 120.0,
            top: false,
            ..Default::default()
        });
        assert_eq!(shell.overlay_size(), Some((400.0, 120.0)));
        shell.hide_overlay();
        assert_eq!(shell.overlay_size(), None);
    }
}
