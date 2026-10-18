//! UI behavior tests (GPUI test platform, `FakeBackend`). Each test names
//! the PARITY row(s) it covers.

use crate::state::AppState;
use crate::testing::FakeBackend;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    px, size, AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Point, Render,
    TestAppContext, Window, WindowBounds, WindowOptions,
};
use std::sync::Arc;

pub fn setup(cx: &mut TestAppContext, backend: Arc<FakeBackend>) -> Entity<AppState> {
    let state = cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::ui::init(cx);
        guidy_i18n::set_language("en");
        let state = AppState::install(backend, cx);
        crate::ui::overlay::OverlayState::install(cx);
        state
    });
    cx.run_until_parked();
    state
}

pub fn open<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> (AnyWindowHandle, Entity<V>) {
    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(900.), px(2400.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| build(window, cx)),
        )
        .expect("open test window")
    });
    cx.run_until_parked();
    (handle, view)
}

/// Render, run `f` against the window, then settle background work.
pub fn with_window(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut gpui_kit::App),
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        f(window, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

pub fn exists(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

mod general {
    use super::*;
    use crate::ui::pages::general::GeneralPage;

    /// PARITY G-09, W-07
    #[gpui_kit::test]
    fn audio_feedback_toggle_persists_through_the_backend(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, GeneralPage::new);
        assert!(exists(cx, w, "page-general"));
        with_window(cx, w, |window, cx| {
            window.click("audio-feedback-toggle", cx)
        });
        assert!(
            backend.called("update_setting audio_feedback true"),
            "{:?}",
            backend.calls()
        );
        assert!(backend.settings().audio_feedback);
    }
}

fn event(name: &str, payload: serde_json::Value) -> guidy_core::host::Event {
    guidy_core::host::Event {
        name: name.into(),
        target: None,
        payload,
    }
}

pub fn dispatch(
    cx: &mut TestAppContext,
    state: &Entity<AppState>,
    name: &str,
    payload: serde_json::Value,
) {
    let ev = event(name, payload);
    cx.update(|cx| {
        let overlay = crate::ui::overlay::OverlayState::global(cx).unwrap();
        crate::dispatch_event(&ev, state, &overlay, cx);
    });
    cx.run_until_parked();
}

fn model(id: &str) -> guidy_core::managers::model::ModelInfo {
    crate::logic::models::test_model(id)
}

mod general_page {
    use super::*;
    use crate::logic::keyboard::OsType;
    use crate::ui::pages::general::GeneralPage;

    /// PARITY G-01, G-02, G-03
    #[gpui_kit::test]
    fn shortcut_rows(cx: &mut TestAppContext) {
        setup(cx, FakeBackend::new());
        let (w, _) = open(cx, GeneralPage::new);
        assert!(exists(cx, w, "shortcut-transcribe"));
        assert!(exists(cx, w, "shortcut-activation"));
        assert_eq!(
            exists(cx, w, "shortcut-cancel"),
            OsType::current() != OsType::Linux
        );
    }

    /// PARITY G-08
    #[gpui_kit::test]
    fn channel_selector_only_for_multichannel_devices(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, GeneralPage::new);
        assert!(!exists(cx, w, "channel"));
        backend.data.lock().channels = 2;
        backend.with_settings(|s| s.selected_microphone = Some("Studio Mic".into()));
        let state = cx.update(|cx| AppState::global(cx));
        state.update(cx, |s, cx| s.refresh_settings(cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "channel"));
    }

    /// PARITY G-10
    #[gpui_kit::test]
    fn output_device_and_volume_follow_audio_feedback(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, GeneralPage::new);
        // Startup already loaded the device lists once.
        backend.calls.lock().clear();
        // Disabled while feedback is off: the menu doesn't open.
        with_window(cx, w, |window, cx| {
            window.click("output-device-dropdown", cx)
        });
        assert!(!backend.called("get_available_output_devices"));
        with_window(cx, w, |window, cx| {
            window.click("audio-feedback-toggle", cx)
        });
        assert!(backend.settings().audio_feedback);
        assert!(exists(cx, w, "slider-audio_feedback_volume"));
    }

    /// PARITY G-04, G-05, G-06
    #[gpui_kit::test]
    fn model_settings_card_follows_model_capabilities(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let mut m = model("multi");
        m.is_downloaded = true;
        m.supports_language_selection = false;
        m.supports_translation = false;
        backend.data.lock().models = vec![m.clone()];
        backend.data.lock().current_model = "multi".into();
        setup(cx, backend.clone());
        let (w, _) = open(cx, GeneralPage::new);
        assert!(!exists(cx, w, "language-selector"));
        m.supports_language_selection = true;
        m.supports_translation = true;
        backend.data.lock().models = vec![m];
        let state = cx.update(|cx| AppState::global(cx));
        state.update(cx, |s, cx| s.load_models(cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "language-selector"));
        assert!(exists(cx, w, "translate-to-english"));
        with_window(cx, w, |window, cx| window.click("translate-toggle", cx));
        assert!(backend.called("update_setting translate_to_english true"));
    }

    /// PARITY W-07 (rollback)
    #[gpui_kit::test]
    fn failed_setting_rolls_back(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().fail_settings = vec!["mute_while_recording".into()];
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, GeneralPage::new);
        with_window(cx, w, |window, cx| window.click("mute-toggle", cx));
        assert!(backend.called("update_setting mute_while_recording true"));
        let value = state.read_with(cx, |s, _| s.app_settings().unwrap().mute_while_recording);
        assert!(!value, "optimistic value rolled back");
        let checked = cx
            .update_window(w, |_, window, cx| {
                window.render_frame(cx);
                window.find("mute-toggle").checked()
            })
            .unwrap();
        assert_eq!(checked, Some(false));
    }
}

mod main_window {
    use super::*;
    use crate::logic::flows::OnboardingStep;
    use crate::ui::main_window::{MainWindow, Section};
    use std::time::Duration;

    /// PARITY W-01
    #[gpui_kit::test]
    fn sidebar_sections_follow_settings(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Done)
        );
        for id in [
            "nav-general",
            "nav-history",
            "nav-models",
            "nav-advanced",
            "nav-about",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        assert!(!exists(cx, w, "nav-postprocessing"));
        assert!(!exists(cx, w, "nav-debug"));
        backend.with_settings(|s| {
            s.post_process_enabled = true;
            s.debug_mode = true;
        });
        state.update(cx, |s, cx| s.refresh_settings(cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "nav-postprocessing"));
        assert!(exists(cx, w, "nav-debug"));
    }

    /// PARITY W-01, W-03
    #[gpui_kit::test]
    fn navigation_switches_pages(cx: &mut TestAppContext) {
        setup(cx, FakeBackend::new());
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert!(exists(cx, w, "page-general"));
        with_window(cx, w, |window, cx| window.click("nav-models", cx));
        assert_eq!(main.read_with(cx, |m, _| m.section), Section::Models);
        assert!(exists(cx, w, "page-models"));
        with_window(cx, w, |window, cx| window.click("nav-about", cx));
        assert!(exists(cx, w, "page-about"));
        with_window(cx, w, |window, cx| window.click("nav-history", cx));
        assert!(exists(cx, w, "page-history"));
        with_window(cx, w, |window, cx| window.click("nav-advanced", cx));
        assert!(exists(cx, w, "page-advanced"));
    }

    /// PARITY F-01 (returning user) and C-16
    #[gpui_kit::test]
    fn returning_user_initializes_input_and_devices(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (_w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Done)
        );
        assert!(backend.called("initialize_enigo"));
        assert!(backend.called("initialize_shortcuts"));
        assert!(backend.called("get_available_microphones"));
    }

    /// PARITY F-01 (new user), F-02, F-03
    #[gpui_kit::test]
    fn new_user_goes_through_permissions_then_model_picker(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| s.onboarding_completed = false);
        setup(cx, backend.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Accessibility)
        );
        // Permissions already granted on this fake: completes after 300 ms.
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Model)
        );
        assert!(exists(cx, w, "model-picker"));
    }

    /// PARITY F-02 (waiting + polling)
    #[gpui_kit::test]
    fn permission_grant_polls_until_granted(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| s.onboarding_completed = false);
        backend.data.lock().microphone = false;
        setup(cx, backend.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        if crate::logic::keyboard::OsType::current() != crate::logic::keyboard::OsType::MacOs {
            return;
        }
        assert!(exists(cx, w, "permissions-onboarding"));
        with_window(cx, w, |window, cx| {
            window.click("grant-microphone-permission", cx)
        });
        assert!(backend.called("request_microphone_permission"));
        backend.data.lock().microphone = true;
        cx.executor().advance_clock(Duration::from_millis(1100));
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Model)
        );
    }

    /// PARITY W-02
    #[gpui_kit::test]
    fn debug_shortcut_toggles_debug_mode(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, MainWindow::new);
        cx.run_until_parked();
        cx.update_window(w, |_, window, cx| {
            window.dispatch_action(Box::new(crate::ui::ToggleDebugMode), cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(backend.called("update_setting debug_mode true"));
    }

    /// PARITY W-06
    #[gpui_kit::test]
    fn status_bar_shows_model_and_version(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert!(exists(cx, w, "model-selector"));
        assert!(exists(cx, w, "update-status"));
        assert!(exists(cx, w, "app-version"));
    }

    /// PARITY D-02 (onboarding preview)
    #[gpui_kit::test]
    fn onboarding_preview_is_inert_and_exits(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        main.update(cx, |m, cx| {
            m.start_preview(OnboardingStep::Accessibility, cx)
        });
        cx.run_until_parked();
        assert!(exists(cx, w, "permissions-onboarding"));
        with_window(cx, w, |window, cx| window.click("exit-preview", cx));
        assert!(main.read_with(cx, |m, _| m.preview.is_none()));
        assert!(!backend.called("request_accessibility_permission"));
    }
}

mod advanced_page {
    use super::*;
    use crate::ui::pages::advanced::AdvancedPage;

    /// PARITY A-01, A-02, A-04
    #[gpui_kit::test]
    fn app_group_rows_and_overlay_position_visibility(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        // Linux defaults the overlay off; position only shows when it's on.
        backend.with_settings(|s| s.overlay_style = guidy_core::settings::OverlayStyle::Live);
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, AdvancedPage::new);
        for id in [
            "start-hidden",
            "autostart",
            "show-tray-icon",
            "overlay-style",
            "overlay-position",
            "model-unload",
            "experimental",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        with_window(cx, w, |window, cx| window.click("start-hidden-toggle", cx));
        assert!(backend.called("update_setting start_hidden true"));
        backend.with_settings(|s| s.overlay_style = guidy_core::settings::OverlayStyle::None);
        state.update(cx, |s, cx| s.refresh_settings(cx));
        cx.run_until_parked();
        assert!(!exists(cx, w, "overlay-position"));
    }

    /// PARITY A-13
    #[gpui_kit::test]
    fn experimental_group_only_when_enabled(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, AdvancedPage::new);
        assert!(!exists(cx, w, "keyboard-implementation"));
        with_window(cx, w, |window, cx| window.click("experimental-toggle", cx));
        for id in [
            "post-process-enabled",
            "keyboard-implementation",
            "acceleration",
            "lazy-stream-close",
            "vad-backend",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        with_window(cx, w, |window, cx| window.click("post-process-toggle", cx));
        assert!(backend.called("update_setting post_process_enabled true"));
    }

    /// PARITY A-09, A-11, A-12
    #[gpui_kit::test]
    fn transcription_and_history_rows(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, AdvancedPage::new);
        for id in [
            "vad",
            "long-form-chunking",
            "filler-word-removal",
            "chinese-script",
            "custom-words",
            "append-trailing-space",
            "history-limit",
            "recording-retention",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        with_window(cx, w, |window, cx| window.click("vad-toggle", cx));
        assert!(backend.called("update_setting vad_enabled false"));
    }

    /// PARITY A-10
    #[gpui_kit::test]
    fn custom_words_add_and_remove(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, page) = open(cx, AdvancedPage::new);
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| p.type_custom_word("  <Guidy>  ", window, cx));
        })
        .unwrap();
        with_window(cx, w, |window, cx| window.click("custom-words-add", cx));
        assert!(
            backend.called(r#"update_setting custom_words ["Guidy"]"#),
            "{:?}",
            backend.calls()
        );
        assert!(exists(cx, w, "custom-words-list"));
        // duplicate → toast, no write
        let calls = backend.calls().len();
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| p.type_custom_word("Guidy", window, cx));
        })
        .unwrap();
        let toasts = std::rc::Rc::new(std::cell::RefCell::new(0));
        let t2 = toasts.clone();
        let _sub = cx.update(|cx| {
            cx.subscribe(&state, move |_, ev: &crate::state::AppEvent, _| {
                if matches!(ev, crate::state::AppEvent::Toast(_)) {
                    *t2.borrow_mut() += 1;
                }
            })
        });
        with_window(cx, w, |window, cx| window.click("custom-words-add", cx));
        assert_eq!(backend.calls().len(), calls);
        assert_eq!(*toasts.borrow(), 1);
        with_window(cx, w, |window, cx| {
            window
                .within("custom-words-list")
                .click(("custom-word", 0usize), cx)
        });
        assert!(backend.called("update_setting custom_words []"));
    }
}

mod models_page {
    use super::*;
    use crate::ui::pages::models::ModelsPage;
    use guidy_core::managers::model::RetiredModel;

    fn backend_with_models() -> Arc<FakeBackend> {
        let backend = FakeBackend::new();
        let mut active = model("active");
        active.is_downloaded = true;
        let mut other = model("other");
        other.is_downloaded = true;
        let fresh = model("fresh");
        backend.data.lock().models = vec![active, other, fresh];
        backend.data.lock().current_model = "active".into();
        backend
    }

    /// PARITY M-01, M-02
    #[gpui_kit::test]
    fn sections_and_rescan(cx: &mut TestAppContext) {
        let backend = backend_with_models();
        setup(cx, backend.clone());
        let (w, _) = open(cx, ModelsPage::new);
        for id in ["model-card-active", "model-card-other", "model-card-fresh"] {
            assert!(exists(cx, w, id), "{id}");
        }
        with_window(cx, w, |window, cx| window.click("models-rescan", cx));
        assert!(backend.called("rescan_local_models"));
    }

    /// PARITY M-03, C-10
    #[gpui_kit::test]
    fn card_clicks_download_or_select(cx: &mut TestAppContext) {
        let backend = backend_with_models();
        setup(cx, backend.clone());
        let (w, _) = open(cx, ModelsPage::new);
        with_window(cx, w, |window, cx| window.click("model-card-active", cx));
        assert!(
            !backend.called("set_active_model"),
            "active card is not clickable"
        );
        with_window(cx, w, |window, cx| window.click("model-card-fresh", cx));
        assert!(backend.called("download_model fresh"));
        with_window(cx, w, |window, cx| window.click("model-card-other", cx));
        assert!(backend.called("set_active_model other"));
    }

    /// PARITY M-05, E-08
    #[gpui_kit::test]
    fn download_progress_and_cancel(cx: &mut TestAppContext) {
        let backend = backend_with_models();
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, ModelsPage::new);
        with_window(cx, w, |window, cx| window.click("model-card-fresh", cx));
        dispatch(
            cx,
            &state,
            "model-download-progress",
            serde_json::json!({"model_id": "fresh", "downloaded": 10, "total": 100, "percentage": 10.0}),
        );
        assert!(exists(cx, w, "cancel-fresh"));
        with_window(cx, w, |window, cx| window.click("cancel-fresh", cx));
        assert!(backend.called("cancel_download fresh"));
    }

    /// PARITY M-06
    #[gpui_kit::test]
    fn delete_asks_for_confirmation(cx: &mut TestAppContext) {
        let backend = backend_with_models();
        setup(cx, backend.clone());
        let (w, _) = open(cx, ModelsPage::new);
        with_window(cx, w, |window, cx| window.click("delete-other", cx));
        let open_dialog = cx
            .update_window(w, |_, window, cx| {
                gpui_kit::component::WindowExt::has_active_dialog(window, cx)
            })
            .unwrap();
        assert!(open_dialog);
        assert!(!backend.called("delete_model"));
    }

    /// PARITY M-07
    #[gpui_kit::test]
    fn retired_notice_deletes_all_but_current(cx: &mut TestAppContext) {
        let backend = backend_with_models();
        backend.data.lock().retired = vec![RetiredModel {
            id: "old".into(),
            name: "Old".into(),
            size_mb: 100,
            replacement_repo: "org/fresh".into(),
        }];
        setup(cx, backend.clone());
        let (w, _) = open(cx, ModelsPage::new);
        assert!(exists(cx, w, "retired-notice"));
        with_window(cx, w, |window, cx| window.click("retired-delete", cx));
        assert!(backend.called("delete_retired_model old"));
        assert!(!exists(cx, w, "retired-notice"));
    }
}

mod history_page {
    use super::*;
    use crate::testing::history_entry;
    use crate::ui::pages::history::HistoryPage;

    /// PARITY H-01, H-03, H-05, H-06, H-07
    #[gpui_kit::test]
    fn entry_actions(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().history = vec![history_entry(2, "second"), history_entry(1, "")];
        setup(cx, backend.clone());
        let (w, _) = open(cx, HistoryPage::new);
        assert!(exists(cx, w, "history-entry-2"));
        assert!(exists(cx, w, "history-entry-1"));
        with_window(cx, w, |window, cx| window.click("star-2", cx));
        assert!(backend.called("toggle_history_entry_saved 2"));
        with_window(cx, w, |window, cx| window.click("retry-2", cx));
        assert!(backend.called("retry_history_entry_transcription 2"));
        with_window(cx, w, |window, cx| window.click("delete-1", cx));
        assert!(backend.called("delete_history_entry 1"));
        assert!(!exists(cx, w, "history-entry-1"));
    }

    /// PARITY H-04
    #[gpui_kit::test]
    fn copy_writes_clipboard_and_is_disabled_for_empty_text(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().history = vec![history_entry(2, "hello"), history_entry(1, "")];
        setup(cx, backend.clone());
        let (w, _) = open(cx, HistoryPage::new);
        with_window(cx, w, |window, cx| window.click("copy-2", cx));
        let text = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        assert_eq!(text.as_deref(), Some("hello"));
        // Empty text: copy is disabled, the clipboard keeps the old value.
        with_window(cx, w, |window, cx| window.click("copy-1", cx));
        let text = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        assert_eq!(text.as_deref(), Some("hello"));
    }

    /// PARITY E-09
    #[gpui_kit::test]
    fn live_added_entries_prepend(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, HistoryPage::new);
        dispatch(
            cx,
            &state,
            "history-update-payload",
            serde_json::json!({"action": "added", "entry": history_entry(7, "new")}),
        );
        assert!(exists(cx, w, "history-entry-7"));
    }

    /// PARITY H-01 (pagination)
    #[gpui_kit::test]
    fn pages_of_thirty(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().history = (1..=45).rev().map(|i| history_entry(i, "x")).collect();
        setup(cx, backend.clone());
        let (_w, page) = open(cx, HistoryPage::new);
        cx.run_until_parked();
        assert!(backend.called("get_history_entries None 30"));
        let n = page.read_with(cx, |p, _| p.entry_count());
        assert!(n == 30 || n == 45, "first page loaded ({n})");
    }
}

mod post_processing_page {
    use super::*;
    use crate::ui::pages::post_processing::PostProcessingPage;

    /// PARITY P-01, P-02, P-05, P-06
    #[gpui_kit::test]
    fn api_rows(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, PostProcessingPage::new);
        for id in [
            "shortcut-transcribe_with_post_process",
            "pp-provider",
            "pp-api-key",
            "pp-model",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        with_window(cx, w, |window, cx| window.click("pp-refresh-models", cx));
        assert!(backend.called("fetch_post_process_models"));
    }

    /// PARITY P-07
    #[gpui_kit::test]
    fn create_prompt(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, page) = open(cx, PostProcessingPage::new);
        with_window(cx, w, |window, cx| window.click("pp-create-new", cx));
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| {
                p.set_prompt_draft("Polish", "Fix: ${output}", window, cx)
            });
        })
        .unwrap();
        with_window(cx, w, |window, cx| window.click("pp-create-prompt", cx));
        assert!(backend.called("add_post_process_prompt Polish"));
        assert!(backend
            .calls()
            .iter()
            .any(|c| c.starts_with("update_setting post_process_selected_prompt_id")));
    }
}

mod debug_and_about {
    use super::*;
    use crate::ui::pages::{about::AboutPage, debug::DebugPage};

    /// PARITY D-01, D-03, D-04, D-05
    #[gpui_kit::test]
    fn debug_rows(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, DebugPage::new);
        for id in [
            "log-level",
            "update-checks",
            "sound-theme",
            "slider-word_correction_threshold",
            "slider-paste_delay_ms",
            "slider-paste_delay_after_ms",
            "slider-hold_threshold_ms",
            "slider-extra_recording_buffer_ms",
            "always-on-microphone",
            "live-logs-row",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        with_window(cx, w, |window, cx| window.click("sound-theme-play", cx));
        assert!(backend.called("play_test_sound start"));
        assert!(backend.called("play_test_sound stop"));
    }

    /// PARITY D-08, E-15
    #[gpui_kit::test]
    fn live_logs_buffer_and_clear(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, DebugPage::new);
        dispatch(
            cx,
            &state,
            "log://log",
            serde_json::json!({"message": "hello", "level": 3}),
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        assert_eq!(state.read_with(cx, |s, _| s.log_lines.len()), 1);
        with_window(cx, w, |window, cx| window.click("logs-clear", cx));
        assert_eq!(state.read_with(cx, |s, _| s.log_lines.len()), 0);
    }

    /// PARITY D-07
    #[gpui_kit::test]
    fn keyboard_diagnostic_runs_on_mac(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, DebugPage::new);
        if crate::logic::keyboard::OsType::current() != crate::logic::keyboard::OsType::MacOs {
            assert!(!exists(cx, w, "keyboard-diagnostic"));
            return;
        }
        with_window(cx, w, |window, cx| window.click("diag-run", cx));
        assert!(backend.called("run_keyboard_diagnostic 10"));
    }

    /// PARITY B-01, B-02, B-03, B-04, B-05
    #[gpui_kit::test]
    fn about_rows(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, AboutPage::new);
        for id in [
            "app-language",
            "theme",
            "version",
            "whats-new-updates",
            "app-data-dir",
            "log-dir",
            "ack-ggml",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        with_window(cx, w, |window, cx| window.click("open-app-data", cx));
        assert!(backend.called("open_app_data_dir"));
        with_window(cx, w, |window, cx| window.click("open-log-dir", cx));
        assert!(backend.called("open_log_dir"));
        with_window(cx, w, |window, cx| window.click("whats-new-toggle", cx));
        assert!(backend.called("update_setting show_whats_new_on_update false"));
    }
}

mod overlay_view {
    use super::*;
    use crate::ui::overlay::OverlayView;

    /// PARITY O-03, O-09, O-11, E-04
    #[gpui_kit::test]
    fn compact_states_and_cancel(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        backend.with_settings(|s| s.overlay_style = guidy_core::settings::OverlayStyle::Minimal);
        state.update(cx, |s, cx| s.refresh_settings(cx));
        cx.run_until_parked();
        let (w, _) = open(cx, OverlayView::new);
        dispatch(cx, &state, "show-overlay", serde_json::json!("recording"));
        assert!(exists(cx, w, "overlay-stage"));
        assert!(!exists(cx, w, "overlay-work-label"));
        dispatch(
            cx,
            &state,
            "show-overlay",
            serde_json::json!("transcribing"),
        );
        assert!(exists(cx, w, "overlay-work-label"));
        with_window(cx, w, |window, cx| window.click("overlay-cancel", cx));
        assert!(backend.called("cancel_operation"));
        dispatch(cx, &state, "hide-overlay", serde_json::Value::Null);
        assert!(!exists(cx, w, "overlay-stage"));
    }

    /// PARITY O-06, O-08, E-06
    #[gpui_kit::test]
    fn live_card_shows_text_and_timer(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        // The live card needs the live style (Linux defaults the overlay off).
        backend.with_settings(|s| s.overlay_style = guidy_core::settings::OverlayStyle::Live);
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, OverlayView::new);
        dispatch(cx, &state, "show-overlay", serde_json::json!("streaming"));
        dispatch(cx, &state, "recording-ready", serde_json::Value::Null);
        assert!(
            !exists(cx, w, "overlay-text"),
            "collapsed until text arrives"
        );
        dispatch(
            cx,
            &state,
            "stream-text-event",
            serde_json::json!({"committed": "hello", "tentative": "wor"}),
        );
        assert!(exists(cx, w, "overlay-text"));
        assert!(exists(cx, w, "overlay-timer"));
    }
}

mod shortcut_recorder {
    use super::*;
    use crate::ui::shortcut_input::ShortcutInput;

    /// PARITY K-04, C-03
    #[gpui_kit::test]
    fn handy_keys_recording_commits_on_key_release(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.keyboard_implementation = guidy_core::settings::KeyboardImplementation::HandyKeys
        });
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, |window, cx| {
            ShortcutInput::new("transcribe", window, cx)
        });
        with_window(cx, w, |window, cx| {
            window.click("shortcut-chip-transcribe", cx)
        });
        assert!(backend.called("start_handy_keys_recording transcribe"));
        let key = |mods: &[&str], key: Option<&str>, down: bool, s: &str| serde_json::json!({"modifiers": mods, "key": key, "is_key_down": down, "hotkey_string": s});
        dispatch(
            cx,
            &state,
            "handy-keys-event",
            key(&["ctrl"], None, true, "ctrl"),
        );
        dispatch(
            cx,
            &state,
            "handy-keys-event",
            key(&["ctrl"], Some("k"), true, "ctrl+k"),
        );
        dispatch(
            cx,
            &state,
            "handy-keys-event",
            key(&["ctrl"], Some("k"), false, "ctrl+k"),
        );
        assert!(
            backend.called("change_binding transcribe ctrl+k"),
            "{:?}",
            backend.calls()
        );
        assert!(backend.called("stop_handy_keys_recording"));
    }

    /// PARITY K-03
    #[gpui_kit::test]
    fn system_recorder_suspends_bindings(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.keyboard_implementation = guidy_core::settings::KeyboardImplementation::Tauri
        });
        setup(cx, backend.clone());
        let (w, _) = open(cx, |window, cx| {
            ShortcutInput::new("transcribe", window, cx)
        });
        with_window(cx, w, |window, cx| {
            window.click("shortcut-chip-transcribe", cx)
        });
        assert!(backend.called("suspend_all_bindings"));
        with_window(cx, w, |window, cx| window.press("ctrl-j", cx));
        assert!(
            backend.called("change_binding transcribe"),
            "{:?}",
            backend.calls()
        );
        assert!(backend.called("resume_all_bindings"));
    }

    /// PARITY C-01 (reset)
    #[gpui_kit::test]
    fn reset_restores_default(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, |window, cx| {
            ShortcutInput::new("transcribe", window, cx)
        });
        with_window(cx, w, |window, cx| {
            window.click("shortcut-reset-transcribe", cx)
        });
        assert!(backend.called("reset_binding transcribe"));
    }
}

mod onboarding_flows {
    use super::*;
    use crate::logic::flows::OnboardingStep;
    use crate::ui::main_window::MainWindow;
    use guidy_core::managers::model::RetiredModel;

    /// PARITY F-03
    #[gpui_kit::test]
    fn picker_downloads_then_selects_and_finishes(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let mut rec = model("rec");
        rec.is_recommended = true;
        backend.data.lock().models = vec![rec.clone()];
        backend.with_settings(|s| s.onboarding_completed = false);
        let state = setup(cx, backend.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Model)
        );
        with_window(cx, w, |window, cx| window.click("model-card-rec", cx));
        assert!(backend.called("download_model rec"));
        // download completes
        rec.is_downloaded = true;
        backend.data.lock().models = vec![rec];
        dispatch(
            cx,
            &state,
            "model-download-complete",
            serde_json::json!("rec"),
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        cx.run_until_parked();
        assert!(backend.called("set_active_model rec"));
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Done)
        );
    }

    /// PARITY F-04
    #[gpui_kit::test]
    fn retired_model_migration_replaces_and_finishes(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let mut new = model("new");
        new.is_downloaded = true;
        backend.data.lock().models = vec![new];
        backend.data.lock().retired = vec![RetiredModel {
            id: "old".into(),
            name: "Old".into(),
            size_mb: 300,
            replacement_repo: "org/new".into(),
        }];
        backend.with_settings(|s| s.selected_model = "old".into());
        setup(cx, backend.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Migration)
        );
        with_window(cx, w, |window, cx| window.click("migration-start", cx));
        cx.run_until_parked();
        assert!(backend.called("set_active_model new"));
        assert!(backend.called("delete_retired_model old"));
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Done)
        );
    }
}

mod whats_new_gate {
    use super::*;
    use crate::ui::main_window::MainWindow;

    /// PARITY F-05
    #[gpui_kit::test]
    async fn shows_unseen_note_and_records_dismissal(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.show_whats_new_on_update = true;
            s.whats_new_last_seen_version = "0.0.1".into();
        });
        setup(cx, backend.clone());
        let (w, _) = open(cx, MainWindow::new);
        cx.wait_for(w, std::time::Duration::from_secs(1), |window, _| {
            window.try_find("dialog").is_some()
        })
        .await;
        // Confirm (the dialog's primary action) with the keyboard.
        cx.update_window(w, |_, window, cx| window.press("enter", cx))
            .unwrap();
        cx.run_until_parked();
        let still = cx
            .update_window(w, |_, window, cx| {
                gpui_kit::component::WindowExt::has_active_dialog(window, cx)
            })
            .unwrap();
        assert!(!still, "dialog dismissed");
        cx.run_until_parked();
        assert!(
            backend.called(r#"update_setting whats_new_last_seen_version "0.1.0""#),
            "{:?}",
            backend.calls()
        );
    }

    /// PARITY F-05 (disabled)
    #[gpui_kit::test]
    fn respects_the_toggle(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.show_whats_new_on_update = false;
            s.whats_new_last_seen_version = "0.0.1".into();
        });
        setup(cx, backend.clone());
        let (w, _) = open(cx, MainWindow::new);
        cx.run_until_parked();
        let open_dialog = cx
            .update_window(w, |_, window, cx| {
                gpui_kit::component::WindowExt::has_active_dialog(window, cx)
            })
            .unwrap();
        assert!(!open_dialog);
    }
}

mod banners_and_events {
    use super::*;
    use crate::logic::keyboard::OsType;
    use crate::ui::banners::{AccessibilityBanner, SecureInputBanner};
    use crate::ui::status_bar::StatusBar;

    /// PARITY W-04
    #[gpui_kit::test]
    fn accessibility_banner_when_permission_missing(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().accessibility = false;
        setup(cx, backend.clone());
        let (w, _) = open(cx, AccessibilityBanner::new);
        cx.run_until_parked();
        let mac = OsType::current() == OsType::MacOs;
        assert_eq!(exists(cx, w, "accessibility-banner"), mac);
        if mac {
            with_window(cx, w, |window, cx| {
                window.click("accessibility-open-settings", cx)
            });
            assert!(backend.called("request_accessibility_permission"));
            backend.data.lock().accessibility = true;
            with_window(cx, w, |window, cx| {
                window.click("accessibility-open-settings", cx)
            });
            assert!(
                !exists(cx, w, "accessibility-banner"),
                "verify hides it once granted"
            );
        }
    }

    /// PARITY W-05, E-12
    #[gpui_kit::test]
    fn secure_input_banner_follows_events_and_dismisses_per_episode(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, |_, cx| SecureInputBanner::new(cx));
        assert!(!exists(cx, w, "secure-input-banner"));
        let impacted = serde_json::json!({
            "enabled": true, "sustained": true, "culprit_pid": 42, "culprit_name": "Terminal",
            "fallback_active": true, "covered_bindings": [], "degraded_bindings": [],
            "uncovered_bindings": ["transcribe"], "recorder_blocked": false
        });
        dispatch(cx, &state, "secure-input-changed", impacted.clone());
        assert!(exists(cx, w, "secure-input-banner"));
        with_window(cx, w, |window, cx| window.click("secure-input-dismiss", cx));
        assert!(!exists(cx, w, "secure-input-banner"));
        let cleared = serde_json::json!({
            "enabled": false, "sustained": false, "culprit_pid": null, "culprit_name": null,
            "fallback_active": false, "covered_bindings": [], "degraded_bindings": [],
            "uncovered_bindings": [], "recorder_blocked": false
        });
        dispatch(cx, &state, "secure-input-changed", cleared);
        dispatch(cx, &state, "secure-input-changed", impacted);
        assert!(
            exists(cx, w, "secure-input-banner"),
            "a new episode warns again"
        );
    }

    /// PARITY E-14, F-06
    #[gpui_kit::test]
    fn tray_update_check_event_runs_a_manual_check(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (_w, bar) = open(cx, |_, cx| StatusBar::new(cx));
        cx.run_until_parked();
        dispatch(cx, &state, "check-for-updates", serde_json::Value::Null);
        cx.run_until_parked();
        let status = bar.read_with(cx, |b, _| b.update_status());
        assert!(status.enabled);
        assert!(
            status.show_up_to_date,
            "manual check with no update shows 'up to date'"
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(4));
        cx.run_until_parked();
        assert!(
            !bar.read_with(cx, |b, _| b.update_status()).show_up_to_date,
            "cleared after 3 s"
        );
    }

    /// PARITY H-02
    #[gpui_kit::test]
    fn history_open_folder(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (w, _) = open(cx, crate::ui::pages::history::HistoryPage::new);
        with_window(cx, w, |window, cx| window.click("open-recordings", cx));
        assert!(backend.called("open_recordings_folder"));
    }
}

/// Regressions found in the running app (startup without a
/// window, failed downloads, write ordering, recorder cleanup, footer status).
mod gap_regressions {
    use super::*;
    use crate::logic::flows::OnboardingStep;
    use crate::logic::models::SelectorStatus;
    use crate::ui::main_window::MainWindow;
    use crate::ui::shortcut_input::ShortcutInput;
    use guidy_core::managers::model::RetiredModel;
    use serde_json::json;
    use std::time::Duration;

    fn index_of(backend: &FakeBackend, prefix: &str) -> usize {
        backend
            .calls()
            .iter()
            .position(|c| c.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix} not called: {:?}", backend.calls()))
    }

    /// PARITY F-01, S-07 (start hidden: no window is ever opened)
    #[gpui_kit::test]
    fn startup_initializes_dictation_without_a_window(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        assert_eq!(
            state.read_with(cx, |s, _| s.startup.step),
            Some(OnboardingStep::Done)
        );
        assert!(backend.called("initialize_enigo"));
        assert!(backend.called("initialize_shortcuts"));
        assert!(backend.called("get_available_microphones"));
    }

    /// PARITY F-01 (one check per session, not per window open)
    #[gpui_kit::test]
    fn reopening_the_window_keeps_the_session_state(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (_w, _) = open(cx, MainWindow::new);
        let (_w2, main) = open(cx, MainWindow::new);
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Done)
        );
        let inits = backend
            .calls()
            .iter()
            .filter(|c| *c == "initialize_shortcuts")
            .count();
        assert_eq!(inits, 1, "{:?}", backend.calls());
    }

    /// PARITY F-03 (a failed download unlocks the picker)
    #[gpui_kit::test]
    fn picker_unlocks_after_a_failed_download(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let mut rec = model("rec");
        rec.is_recommended = true;
        backend.data.lock().models = vec![rec];
        backend.data.lock().fail_download = true;
        backend.with_settings(|s| s.onboarding_completed = false);
        setup(cx, backend.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        assert_eq!(
            main.read_with(cx, |m, cx| m.step(cx)),
            Some(OnboardingStep::Model)
        );
        with_window(cx, w, |window, cx| window.click("model-card-rec", cx));
        with_window(cx, w, |window, cx| window.click("model-card-rec", cx));
        let attempts = backend
            .calls()
            .iter()
            .filter(|c| *c == "download_model rec")
            .count();
        assert_eq!(attempts, 2, "cards are clickable again after the failure");
    }

    /// PARITY F-04 (a failed replacement download offers retry)
    #[gpui_kit::test]
    fn migration_failed_download_offers_retry(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().models = vec![model("new")];
        backend.data.lock().retired = vec![RetiredModel {
            id: "old".into(),
            name: "Old".into(),
            size_mb: 300,
            replacement_repo: "org/new".into(),
        }];
        backend.data.lock().fail_download = true;
        backend.with_settings(|s| s.selected_model = "old".into());
        setup(cx, backend.clone());
        let (w, _) = open(cx, MainWindow::new);
        with_window(cx, w, |window, cx| window.click("migration-start", cx));
        assert!(
            backend.called("download_model new"),
            "{:?}",
            backend.calls()
        );
        assert!(exists(cx, w, "retry-old"), "failed row shows Retry");
        assert!(exists(cx, w, "migration-choose-another-failed"));
        assert!(!backend.called("delete_retired_model"));
    }

    /// PARITY E-08, M-08 (auto-select belongs to the footer selector)
    #[gpui_kit::test]
    fn download_auto_select_waits_for_onboarding(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| s.onboarding_completed = false);
        let state = setup(cx, backend.clone());
        dispatch(cx, &state, "model-download-complete", json!("m"));
        cx.executor().advance_clock(Duration::from_millis(600));
        cx.run_until_parked();
        assert!(!backend.called("set_active_model"), "{:?}", backend.calls());
    }

    /// PARITY F-05 (never over onboarding)
    #[gpui_kit::test]
    fn whats_new_waits_for_onboarding(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.onboarding_completed = false;
            s.show_whats_new_on_update = true;
            s.whats_new_last_seen_version = "0.0.1".into();
        });
        setup(cx, backend.clone());
        let (w, _) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert!(!exists(cx, w, "dialog"));
    }

    /// PARITY A-13 (device, then accelerator; stop at a failure)
    #[gpui_kit::test]
    fn accelerator_writes_run_in_order(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let writes = || {
            vec![
                ("transcribe_gpu_device", json!(1)),
                ("transcribe_accelerator", json!("gpu")),
            ]
        };
        state.update(cx, |s, cx| s.update_settings_in_order(writes(), cx));
        cx.run_until_parked();
        assert!(
            index_of(&backend, "update_setting transcribe_gpu_device")
                < index_of(&backend, "update_setting transcribe_accelerator")
        );
        backend.calls.lock().clear();
        backend.data.lock().fail_settings = vec!["transcribe_gpu_device".into()];
        state.update(cx, |s, cx| s.update_settings_in_order(writes(), cx));
        cx.run_until_parked();
        assert!(!backend.called("update_setting transcribe_accelerator"));
    }

    /// PARITY A-08 (key first, then enable)
    #[gpui_kit::test]
    fn auto_submit_writes_run_in_order(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let writes = crate::logic::flows::auto_submit_writes("ctrl_enter", false);
        state.update(cx, |s, cx| s.update_settings_in_order(writes, cx));
        cx.run_until_parked();
        assert!(
            index_of(&backend, "update_setting auto_submit_key")
                < index_of(&backend, "update_setting auto_submit true")
        );
        let s = backend.settings();
        assert!(s.auto_submit);
    }

    /// PARITY P-04 (a new endpoint clears the stored model)
    #[gpui_kit::test]
    fn base_url_change_clears_the_model(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        state.update(cx, |s, cx| {
            s.update_post_process("base_url", "custom", "http://localhost:1234".into(), cx)
        });
        cx.run_until_parked();
        assert!(
            index_of(&backend, "change_post_process_base_url custom")
                < index_of(&backend, "change_post_process_model custom ")
        );
    }

    /// PARITY P-07 (the new prompt stays selected)
    #[gpui_kit::test]
    fn created_prompt_stays_selected(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, page) = open(
            cx,
            crate::ui::pages::post_processing::PostProcessingPage::new,
        );
        with_window(cx, w, |window, cx| window.click("pp-create-new", cx));
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| {
                p.set_prompt_draft("Tidy", "Tidy: ${output}", window, cx)
            });
        })
        .unwrap();
        with_window(cx, w, |window, cx| window.click("pp-create-prompt", cx));
        let selected = state.read_with(cx, |s, _| {
            s.app_settings()
                .and_then(|s| s.post_process_selected_prompt_id.clone())
        });
        assert_eq!(selected.as_deref(), Some("prompt_Tidy"));
    }

    /// PARITY K-03 (resume only after the binding write)
    #[gpui_kit::test]
    fn system_recorder_resumes_after_the_binding_write(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.keyboard_implementation = guidy_core::settings::KeyboardImplementation::Tauri
        });
        setup(cx, backend.clone());
        let (w, _) = open(cx, |window, cx| {
            ShortcutInput::new("transcribe", window, cx)
        });
        with_window(cx, w, |window, cx| {
            window.click("shortcut-chip-transcribe", cx)
        });
        with_window(cx, w, |window, cx| window.press("ctrl-j", cx));
        assert!(
            index_of(&backend, "change_binding transcribe")
                < index_of(&backend, "resume_all_bindings")
        );
    }

    /// PARITY K-03 (closing mid-recording gives the shortcuts back)
    #[gpui_kit::test]
    fn closing_while_recording_resumes_bindings(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.keyboard_implementation = guidy_core::settings::KeyboardImplementation::Tauri
        });
        setup(cx, backend.clone());
        let (w, _) = open(cx, |window, cx| {
            ShortcutInput::new("transcribe", window, cx)
        });
        with_window(cx, w, |window, cx| {
            window.click("shortcut-chip-transcribe", cx)
        });
        assert!(backend.called("suspend_all_bindings"));
        cx.update_window(w, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert!(
            backend.called("resume_all_bindings"),
            "{:?}",
            backend.calls()
        );
    }

    /// PARITY M-08 (status follows the current model; load errors shown)
    #[gpui_kit::test]
    fn selector_status_follows_the_current_model(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().current_model = "a".into();
        backend.data.lock().loaded_model = Some("a".into());
        let state = setup(cx, backend.clone());
        assert_eq!(
            state.read_with(cx, |s, _| s.selector_status),
            SelectorStatus::Ready
        );
        // The active model was deleted.
        backend.data.lock().current_model = String::new();
        state.update(cx, |s, cx| s.load_current_model(cx));
        cx.run_until_parked();
        assert_eq!(
            state.read_with(cx, |s, _| s.selector_status),
            SelectorStatus::None
        );
        dispatch(
            cx,
            &state,
            "model-state-changed",
            json!({"event_type": "loading_failed", "error": "bad gguf"}),
        );
        assert_eq!(
            state.read_with(cx, |s, _| s.selector_error.clone()),
            Some("bad gguf".into())
        );
    }

    /// PARITY G-07 (device list failure keeps "Default" usable)
    #[gpui_kit::test]
    fn device_list_failure_falls_back_to_default(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.data.lock().fail_devices = true;
        let state = setup(cx, backend.clone());
        let names: Vec<String> = state.read_with(cx, |s, _| {
            s.audio_devices.iter().map(|d| d.name.clone()).collect()
        });
        assert_eq!(names, vec!["Default".to_string()]);
    }
}
