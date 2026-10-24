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

    #[gpui_kit::test]
    fn usage_page_lists_spend_per_model(cx: &mut TestAppContext) {
        use crate::ui::pages::usage::UsagePage;
        use guidy_core::llm::usage::{
            FeatureSpend, LocalKind, LocalModelUsage, LocalUsageReport, ModelSpend, UsageFeature,
            UsageReport,
        };
        let backend = FakeBackend::new();
        backend.data.lock().usage = UsageReport {
            models: vec![ModelSpend {
                provider_id: "anthropic".into(),
                model: "claude-sonnet-5-5".into(),
                calls: 3,
                input_tokens: 30_000,
                output_tokens: 2_000,
                total_usd: 0.09,
                avg_usd: Some(0.03),
                max_usd: Some(0.05),
                estimated_calls: 3,
                ..Default::default()
            }],
            features: vec![FeatureSpend {
                feature: UsageFeature::MeetingSummary,
                calls: 3,
                total_usd: 0.09,
            }],
            total_usd: 0.09,
            calls: 3,
            last_30_days_usd: 0.09,
            local: LocalUsageReport {
                models: vec![
                    LocalModelUsage {
                        kind: LocalKind::Transcription,
                        model: "Nemotron Streaming 3.5".into(),
                        runs: 12,
                        audio_ms: 3_600_000,
                        timed_audio_ms: 3_600_000,
                        compute_ms: 150_000,
                        words: 9_000,
                        loads: 1,
                        load_ms: 1_200,
                        ..Default::default()
                    },
                    LocalModelUsage {
                        kind: LocalKind::Llm,
                        model: "Qwen3 4B".into(),
                        runs: 2,
                        input_tokens: 9_000,
                        output_tokens: 600,
                        compute_ms: 30_000,
                        prompt_ms: 10_000,
                        ..Default::default()
                    },
                ],
                runs: 14,
                audio_ms: 3_600_000,
                words: 9_000,
                tokens: 9_600,
                ..Default::default()
            },
        };
        setup(cx, backend.clone());
        let (w, page) = open(cx, UsagePage::new);
        cx.run_until_parked();
        assert!(exists(cx, w, "usage-model-anthropic-claude-sonnet-5-5"));
        assert!(!exists(cx, w, "usage-empty"));
        assert!(exists(
            cx,
            w,
            "usage-local-transcription-Nemotron Streaming 3.5"
        ));
        assert!(exists(cx, w, "usage-local-llm-Qwen3 4B"));
        assert!(!exists(cx, w, "usage-local-empty"));
        page.update(cx, |p, cx| p.clear(cx));
        cx.run_until_parked();
        assert!(backend.called("clear_llm_usage"));
        assert!(exists(cx, w, "usage-empty"));
        assert!(exists(cx, w, "usage-local-empty"));
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
            "author",
            "author-site",
            "author-github",
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
    fn native_keys_recording_commits_on_key_release(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.keyboard_implementation = guidy_core::settings::KeyboardImplementation::NativeKeys
        });
        let state = setup(cx, backend.clone());
        let (w, _) = open(cx, |window, cx| {
            ShortcutInput::new("transcribe", window, cx)
        });
        with_window(cx, w, |window, cx| {
            window.click("shortcut-chip-transcribe", cx)
        });
        assert!(backend.called("start_native_keys_recording transcribe"));
        let key = |mods: &[&str], key: Option<&str>, down: bool, s: &str| serde_json::json!({"modifiers": mods, "key": key, "is_key_down": down, "hotkey_string": s});
        dispatch(
            cx,
            &state,
            "native-keys-event",
            key(&["ctrl"], None, true, "ctrl"),
        );
        dispatch(
            cx,
            &state,
            "native-keys-event",
            key(&["ctrl"], Some("k"), true, "ctrl+k"),
        );
        dispatch(
            cx,
            &state,
            "native-keys-event",
            key(&["ctrl"], Some("k"), false, "ctrl+k"),
        );
        assert!(
            backend.called("change_binding transcribe ctrl+k"),
            "{:?}",
            backend.calls()
        );
        assert!(backend.called("stop_native_keys_recording"));
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

/// Like [`setup`], with a meetings fake installed next to the backend.
pub fn setup_meetings(
    cx: &mut TestAppContext,
    backend: Arc<FakeBackend>,
    meetings: Arc<crate::testing::FakeMeetingsBackend>,
) -> Entity<AppState> {
    let state = cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::ui::init(cx);
        guidy_i18n::set_language("en");
        let state = AppState::install_with_meetings(backend, meetings, cx);
        crate::ui::overlay::OverlayState::install(cx);
        state
    });
    cx.run_until_parked();
    state
}

mod meetings_page {
    use super::*;
    use crate::testing::{
        meeting_fixture, person_fixture, segment_fixture, speaker_fixture, FakeMeetingsBackend,
    };
    use crate::ui::main_window::MainWindow;
    use crate::ui::pages::meetings::MeetingsPage;
    use guidy_core::meetings::types::{
        events, CalendarEvent, MeetingStatus, ProcessingProgress, ProcessingStage,
    };

    fn has_dialog(cx: &mut TestAppContext, w: AnyWindowHandle) -> bool {
        cx.update_window(w, |_, window, cx| {
            gpui_kit::component::WindowExt::has_active_dialog(window, cx)
        })
        .unwrap()
    }

    fn press_enter(cx: &mut TestAppContext, w: AnyWindowHandle) {
        cx.update_window(w, |_, window, cx| window.press("enter", cx))
            .unwrap();
        cx.run_until_parked();
    }

    /// Meetings 1 (processed, two speakers) and 2 (recorded); people Ada, Bob.
    fn fixture() -> (Arc<FakeBackend>, Arc<FakeMeetingsBackend>) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| s.meetings.enabled = true);
        let m = FakeMeetingsBackend::new();
        {
            let mut d = m.data.lock();
            d.meetings = vec![
                meeting_fixture(1, "Weekly sync", MeetingStatus::Processed),
                meeting_fixture(2, "Design review", MeetingStatus::Recorded),
            ];
            d.people = vec![
                person_fixture(7, "Ada", Some("ada@example.com")),
                person_fixture(8, "Bob", None),
            ];
            d.speakers.insert(
                1,
                vec![
                    speaker_fixture(5, 1, "Speaker 1", None, false),
                    speaker_fixture(6, 1, "Speaker 2", Some(7), false),
                ],
            );
            d.segments.insert(
                1,
                vec![
                    segment_fixture(1, 1, 5, 0, "Hello everyone"),
                    segment_fixture(2, 1, 5, 2_000, "Let's start"),
                    segment_fixture(3, 1, 6, 4_000, "Sounds good"),
                ],
            );
        }
        (backend, m)
    }

    #[gpui_kit::test]
    fn page_renders_list_and_records(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        for id in [
            "page-meetings",
            "meeting-recorder",
            "meeting-1",
            "meeting-2",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        // Recorded meetings can be processed.
        with_window(cx, w, |window, cx| window.click("meeting-process-2", cx));
        assert!(meetings.called("process_meeting 2"));

        with_window(cx, w, |window, cx| {
            window.click("meeting-record-toggle", cx)
        });
        assert!(
            meetings.called("start_recording none"),
            "{:?}",
            meetings.calls()
        );
        assert!(exists(cx, w, "meeting-recording-title"));
        with_window(cx, w, |window, cx| {
            window.click("meeting-record-toggle", cx)
        });
        assert!(meetings.called("stop_recording"));
        assert!(!exists(cx, w, "meeting-recording-title"));
    }

    #[gpui_kit::test]
    fn upcoming_event_starts_linked_recording(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().upcoming = vec![CalendarEvent {
            id: "ev-1".into(),
            title: "Standup".into(),
            // Starts in ten minutes: ended events drop off the list.
            start_at: guidy_core::meetings::recorder::now_ms() + 600_000,
            end_at: guidy_core::meetings::recorder::now_ms() + 1_500_000,
            meeting_url: Some("https://meet.google.com/abc".into()),
            attendees: vec![],
        }];
        setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        assert!(exists(cx, w, "upcoming-0"));
        with_window(cx, w, |window, cx| window.click("upcoming-start-0", cx));
        assert!(meetings.called("start_recording ev-1"));
        // While recording, the upcoming list is hidden.
        assert!(!exists(cx, w, "upcoming-0"));
    }

    #[gpui_kit::test]
    fn progress_events_update_the_list(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        assert!(!exists(cx, w, "meeting-cancel-2"));
        let p = ProcessingProgress {
            meeting_id: 2,
            stage: ProcessingStage::Transcribe,
            fraction: 0.4,
            message: None,
        };
        dispatch(
            cx,
            &state,
            events::PROCESSING_PROGRESS,
            serde_json::to_value(&p).unwrap(),
        );
        // The meeting shows as processing (cancel action, no process action).
        assert!(exists(cx, w, "meeting-cancel-2"));
        assert!(!exists(cx, w, "meeting-process-2"));
        // A changed meeting reloads the list.
        meetings.data.lock().meetings.pop();
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(2));
        let n = page.read_with(cx, |p, cx| p.list().read(cx).meeting_count());
        assert_eq!(n, 1);
    }

    #[gpui_kit::test]
    fn renames_and_deletes_meetings(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-rename-1", cx));
        let list = page.read_with(cx, |p, _| p.list().clone());
        cx.update_window(w, |_, window, cx| {
            list.update(cx, |l, cx| l.type_rename("Planning", window, cx))
        })
        .unwrap();
        with_window(cx, w, |window, cx| {
            window.click("meeting-rename-save-1", cx)
        });
        assert!(meetings.called("rename_meeting 1 Planning"));

        with_window(cx, w, |window, cx| window.click("meeting-delete-2", cx));
        assert!(has_dialog(cx, w));
        assert!(!meetings.called("delete_meeting"));
        press_enter(cx, w);
        assert!(
            meetings.called("delete_meeting 2"),
            "{:?}",
            meetings.calls()
        );
        assert!(!exists(cx, w, "meeting-2"));
    }

    #[gpui_kit::test]
    fn speakers_can_be_named_and_confirmed(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        // The speakers are folded when the meeting opens.
        assert!(!exists(cx, w, "speaker-5"));
        with_window(cx, w, |window, cx| {
            window.click("meeting-speakers-fold", cx)
        });
        for id in [
            "meeting-detail",
            "segment-1",
            "segment-3",
            "block-0",
            "block-1",
            "speaker-5",
            "speaker-uncertain-6",
        ] {
            assert!(exists(cx, w, id), "{id}");
        }
        // Two segments by Speaker 1 form one block.
        assert!(!exists(cx, w, "block-2"));

        // Uncertain match: confirm.
        with_window(cx, w, |window, cx| window.click("speaker-confirm-6", cx));
        assert!(meetings.called("confirm_speaker 6"));
        assert!(!exists(cx, w, "speaker-uncertain-6"));

        // Speaker 1 -> a new person with an email.
        with_window(cx, w, |window, cx| window.click("speaker-name-5", cx));
        assert!(meetings.called("speaker_suggestions 5"));
        assert!(exists(cx, w, "speaker-picker-5"));
        assert!(exists(cx, w, "speaker-suggestion-0"));
        let detail = page.read_with(cx, |p, _| p.detail().cloned().unwrap());
        cx.update_window(w, |_, window, cx| {
            detail.update(cx, |d, cx| {
                d.type_new_person("Grace", "grace@example.com", window, cx)
            })
        })
        .unwrap();
        with_window(cx, w, |window, cx| window.click("speaker-new-save", cx));
        assert!(
            meetings.called("assign_speaker 5 new Grace grace@example.com"),
            "{:?}",
            meetings.calls()
        );
        assert!(!exists(cx, w, "speaker-picker-5"));
        let named = detail.read_with(cx, |d, _| {
            d.speakers()
                .iter()
                .find(|s| s.id == 5)
                .and_then(|s| s.person_id)
        });
        assert!(named.is_some());

        // Rename again from a suggestion (Ada, a known person).
        with_window(cx, w, |window, cx| window.click("block-0-speaker", cx));
        assert!(exists(cx, w, "speaker-picker-5"));
        with_window(cx, w, |window, cx| window.click("speaker-suggestion-0", cx));
        assert!(meetings.called("assign_speaker 5 existing 7"));

        // Segment playback decodes that stretch of the track.
        with_window(cx, w, |window, cx| window.click("segment-3", cx));
        assert!(meetings.called("segment_audio 1 system 4000 6000"));

        with_window(cx, w, |window, cx| window.click("meeting-back", cx));
        assert!(!exists(cx, w, "meeting-detail"));
        assert!(exists(cx, w, "meeting-1"));
    }

    #[gpui_kit::test]
    fn people_can_be_edited_and_deleted(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meetings-tab-people", cx));
        assert!(exists(cx, w, "person-7"));
        assert!(exists(cx, w, "person-8"));

        with_window(cx, w, |window, cx| window.click("person-edit-8", cx));
        let people = page.read_with(cx, |p, _| p.people().clone());
        cx.update_window(w, |_, window, cx| {
            people.update(cx, |p, cx| {
                p.type_edit("Robert", "bob@example.com", window, cx)
            })
        })
        .unwrap();
        with_window(cx, w, |window, cx| window.click("person-save-8", cx));
        assert!(meetings.called("rename_person 8 Robert bob@example.com"));

        with_window(cx, w, |window, cx| window.click("person-merge-8", cx));
        assert!(exists(cx, w, "person-merge-panel-8"));

        with_window(cx, w, |window, cx| window.click("person-delete-7", cx));
        assert!(has_dialog(cx, w));
        assert!(!meetings.called("delete_person"));
        press_enter(cx, w);
        assert!(meetings.called("delete_person 7"), "{:?}", meetings.calls());
        assert!(!exists(cx, w, "person-7"));
        assert_eq!(people.read_with(cx, |p, _| p.people_count()), 1);
    }

    #[gpui_kit::test]
    fn the_users_voice_can_be_recorded_and_deleted(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meetings-tab-people", cx));
        assert!(exists(cx, w, "people-voice-record"));

        // Cancel discards the recording.
        with_window(cx, w, |window, cx| window.click("people-voice-record", cx));
        assert!(meetings.called("start_voice_recording"));
        assert!(exists(cx, w, "people-voice-done"));
        with_window(cx, w, |window, cx| window.click("people-voice-cancel", cx));
        assert!(meetings.called("cancel_voice_recording"));
        assert!(!exists(cx, w, "people-voice-done"));

        // Done (enabled after a while) stores it.
        with_window(cx, w, |window, cx| window.click("people-voice-record", cx));
        let people = page.read_with(cx, |p, _| p.people().clone());
        people.update(cx, |p, cx| p.finish_voice_recording(cx));
        cx.run_until_parked();
        assert!(meetings.called("finish_voice_recording"));
        assert!(exists(cx, w, "people-voice-delete"));

        with_window(cx, w, |window, cx| window.click("people-voice-delete", cx));
        assert!(has_dialog(cx, w));
        press_enter(cx, w);
        assert!(
            meetings.called("delete_user_voice"),
            "{:?}",
            meetings.calls()
        );
        assert!(!exists(cx, w, "people-voice-delete"));
    }

    /// The merge panel plays both people's voiceprints before merging.
    #[gpui_kit::test]
    fn voiceprints_can_be_heard_before_a_merge(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        {
            let mut d = meetings.data.lock();
            let segments = d.segments[&1].clone();
            use guidy_core::meetings::types::VoiceSample;
            let sample = |id: i64, seg: usize| VoiceSample {
                voiceprint_id: id,
                meeting_id: Some(1),
                meeting_title: Some("Weekly sync".into()),
                meeting_started_at: Some(0),
                segment: Some(segments[seg].clone()),
                has_audio: true,
            };
            d.voice_samples.insert(8, vec![sample(80, 0)]);
            d.voice_samples.insert(
                7,
                vec![
                    sample(70, 2),
                    VoiceSample {
                        has_audio: false,
                        ..sample(71, 1)
                    },
                ],
            );
        }
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meetings-tab-people", cx));
        with_window(cx, w, |window, cx| window.click("person-merge-8", cx));
        assert!(meetings.called("voice_samples 8"));
        assert!(exists(cx, w, "voice-sample-80"));
        assert!(!exists(cx, w, "voice-sample-70"));

        with_window(cx, w, |window, cx| window.click("voice-sample-80", cx));
        assert!(meetings.called("segment_audio 1 system 0 2000"));

        let people = page.read_with(cx, |p, _| p.people().clone());
        people.update(cx, |p, cx| p.pick_merge_target(7, cx));
        cx.run_until_parked();
        assert!(meetings.called("voice_samples 7"));
        assert!(exists(cx, w, "voice-sample-70"));
        // Its audio is gone: nothing to play.
        with_window(cx, w, |window, cx| window.click("voice-sample-71", cx));
        assert!(!meetings.called("segment_audio 1 system 2000"));

        with_window(cx, w, |window, cx| {
            window.click("person-merge-confirm-8", cx)
        });
        assert!(has_dialog(cx, w));
        assert!(!meetings.called("merge_people"));
        press_enter(cx, w);
        assert!(
            meetings.called("merge_people 8 7"),
            "{:?}",
            meetings.calls()
        );
    }

    #[gpui_kit::test]
    fn merging_moves_into_the_target(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (_w, page) = open(cx, MeetingsPage::new);
        let people = page.read_with(cx, |p, _| p.people().clone());
        people.update(cx, |p, cx| p.merge(8, 7, cx));
        cx.run_until_parked();
        assert!(meetings.called("merge_people 8 7"));
        assert_eq!(people.read_with(cx, |p, _| p.people_count()), 1);
    }

    #[gpui_kit::test]
    fn enabling_requests_permissions_and_saves_settings(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        backend.with_settings(|s| s.meetings.enabled = false);
        meetings.data.lock().permissions.calendar_granted = true;
        setup_meetings(cx, backend.clone(), meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| {
            window.click("meetings-tab-settings", cx)
        });
        assert!(exists(cx, w, "meetings-settings"));
        // Grants are loaded (without prompting) as soon as settings open.
        assert!(exists(cx, w, "meetings-permissions"));
        assert!(!meetings.called("request_permissions"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-enabled-toggle", cx)
        });
        assert!(backend.called("update_setting meetings"));
        assert!(backend.settings().meetings.enabled);
        assert!(meetings.called("request_permissions"));
        assert!(exists(cx, w, "meetings-permissions"));

        with_window(cx, w, |window, cx| {
            window.click("meetings-calendar-notifications-toggle", cx)
        });
        assert!(!backend.settings().meetings.calendar_notifications);

        assert!(!exists(cx, w, "meetings-match-threshold"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-advanced-toggle", cx)
        });
        assert!(exists(cx, w, "meetings-match-threshold"));
        assert!(exists(cx, w, "meetings-suggest-threshold"));
    }

    #[gpui_kit::test]
    fn sidebar_shows_meetings_after_history_when_supported(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings);
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert!(exists(cx, w, "nav-meetings"));
        with_window(cx, w, |window, cx| window.click("nav-meetings", cx));
        assert_eq!(
            main.read_with(cx, |m, _| m.section),
            crate::ui::main_window::Section::Meetings
        );
        assert!(exists(cx, w, "page-meetings"));
    }

    #[gpui_kit::test]
    fn sidebar_hides_meetings_when_unsupported(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().supported = false;
        setup_meetings(cx, backend, meetings);
        let (w, _) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert!(exists(cx, w, "nav-history"));
        assert!(!exists(cx, w, "nav-meetings"));
    }

    #[gpui_kit::test]
    fn sidebar_hides_meetings_without_a_meetings_backend(cx: &mut TestAppContext) {
        setup(cx, FakeBackend::new());
        let (w, _) = open(cx, MainWindow::new);
        cx.run_until_parked();
        assert!(!exists(cx, w, "nav-meetings"));
    }

    fn summarized(meetings: &FakeMeetingsBackend, id: i64) {
        meetings.data.lock().ai.summaries.insert(
            id,
            guidy_core::meetings::types::StoredSummary {
                meeting_id: id,
                title: "Sync".into(),
                short_summary: "Talked.".into(),
                decisions: vec![],
                topics: vec![],
                generated_by: "x/y".into(),
                generated_at: 1,
                usage: None,
            },
        );
    }

    #[gpui_kit::test]
    fn search_shows_highlighted_hits_and_opens_the_line(cx: &mut TestAppContext) {
        use guidy_core::meetings::search::{MatchField, SearchHit, SearchResults, Snippet};
        let (backend, meetings) = fixture();
        meetings.data.lock().ai.search.insert(
            "start".into(),
            SearchResults {
                hits: vec![SearchHit {
                    meeting_id: 1,
                    score: 1.0,
                    title: None,
                    snippets: vec![Snippet {
                        field: MatchField::Transcript,
                        text: "Let's start".into(),
                        highlights: std::iter::once(6..11).collect(),
                        segment_id: Some(2),
                        t_ms: Some(2_000),
                    }],
                    transcript_hits: 1,
                }],
                corrections: vec![],
                partial: false,
            },
        );
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        let list = page.read_with(cx, |p, _| p.list().clone());
        cx.update_window(w, |_, window, cx| {
            list.update(cx, |l, cx| l.type_search("start", window, cx))
        })
        .unwrap();
        // Searched after a short typing pause.
        assert!(!meetings.called("search_meetings"));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        assert!(
            meetings.called("search_meetings start"),
            "{:?}",
            meetings.calls()
        );
        assert!(exists(cx, w, "meetings-search-results"));
        assert!(exists(cx, w, "search-hit-1"));
        assert!(!exists(cx, w, "search-hit-2"));
        // The list is replaced by the results.
        assert!(!exists(cx, w, "meeting-2"));
        // A transcript line opens the meeting there.
        with_window(cx, w, |window, cx| window.click("search-hit-1-0", cx));
        let detail = page.read_with(cx, |p, _| p.detail().cloned()).unwrap();
        assert_eq!(detail.read_with(cx, |d, _| d.meeting_id), 1);

        // Nothing found.
        page.update(cx, |p, cx| p.close_meeting(cx));
        cx.update_window(w, |_, window, cx| {
            list.update(cx, |l, cx| l.type_search("zebra", window, cx))
        })
        .unwrap();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        assert!(exists(cx, w, "meetings-search-empty"));
        // Clearing the box brings the list back.
        cx.update_window(w, |_, window, cx| {
            list.update(cx, |l, cx| l.type_search("", window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert!(exists(cx, w, "meeting-2"));
        assert!(list.read_with(cx, |l, _| l.results().is_none()));
    }

    #[gpui_kit::test]
    fn titles_are_generated_once_summarized(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        summarized(&meetings, 1);
        setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        // Meeting 2 has no summary: its title can't be generated yet.
        with_window(cx, w, |window, cx| window.click("meeting-title-2", cx));
        assert!(!meetings.called("generate_title 2"));
        with_window(cx, w, |window, cx| window.click("meeting-title-1", cx));
        assert!(
            meetings.called("generate_title 1 bulk=false"),
            "{:?}",
            meetings.calls()
        );
        assert!(exists(cx, w, "meeting-description-1"));
    }

    #[gpui_kit::test]
    fn generate_titles_titles_every_candidate(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        summarized(&meetings, 1);
        summarized(&meetings, 2);
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        assert!(exists(cx, w, "meetings-titles-generate"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-titles-generate", cx)
        });
        cx.run_until_parked();
        assert!(meetings.called("generate_title 1 bulk=true"));
        assert!(meetings.called("generate_title 2 bulk=true"));
        let list = page.read_with(cx, |p, _| p.list().clone());
        assert!(list.read_with(cx, |l, _| l.bulk().is_none()));
        // Both have descriptions now: nothing left to title.
        assert!(!exists(cx, w, "meetings-titles-generate"));
    }

    #[gpui_kit::test]
    fn detail_generates_the_title_and_shows_the_description(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        // No summary yet: disabled.
        with_window(cx, w, |window, cx| {
            window.click("meeting-detail-generate-title", cx)
        });
        assert!(!meetings.called("generate_title"));
        assert!(!exists(cx, w, "meeting-detail-description"));
        summarized(&meetings, 1);
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(1));
        with_window(cx, w, |window, cx| {
            window.click("meeting-detail-generate-title", cx)
        });
        assert!(meetings.called("generate_title 1 bulk=false"));
        assert!(exists(cx, w, "meeting-detail-description"));
    }
}

/// Review fixes: settings patches, permissions, speaker model, readiness,
/// stale views, model download, deep links, URL checks.
mod meetings_fixes {
    use super::*;
    use crate::logic::meetings::{MeetingLink, MeetingsEvent, MeetingsInit};
    use crate::meetings_backend::MeetingsError;
    use crate::testing::{
        action_item_fixture, meeting_fixture, segment_fixture, speaker_fixture, FakeMeetingsBackend,
    };
    use crate::ui::main_window::{MainWindow, Section};
    use crate::ui::pages::meetings::ai_settings::AiSettingsView;
    use crate::ui::pages::meetings::summary::SummaryView;
    use crate::ui::pages::meetings::MeetingsPage;
    use crate::ui::pages::post_processing::PostProcessingPage;
    use guidy_core::managers::transcription::meeting::DEFAULT_MEETING_MODEL_ID;
    use guidy_core::meetings::processing::SpeakerModelStatus;
    use guidy_core::meetings::types::{events, MeetingStatus, StoredSummary};

    fn fixture() -> (Arc<FakeBackend>, Arc<FakeMeetingsBackend>) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| s.meetings.enabled = true);
        let m = FakeMeetingsBackend::new();
        {
            let mut d = m.data.lock();
            d.meetings = vec![
                meeting_fixture(1, "Weekly sync", MeetingStatus::Processed),
                meeting_fixture(2, "Design review", MeetingStatus::Recorded),
            ];
            d.speakers
                .insert(1, vec![speaker_fixture(5, 1, "Speaker 1", None, false)]);
            d.segments.insert(
                1,
                vec![
                    segment_fixture(1, 1, 5, 0, "Hello everyone"),
                    segment_fixture(2, 1, 5, 60_000, "Next topic"),
                ],
            );
        }
        (backend, m)
    }

    fn settings_tab(cx: &mut TestAppContext, w: AnyWindowHandle) {
        with_window(cx, w, |window, cx| {
            window.click("meetings-tab-settings", cx)
        });
    }

    fn meetings_updates(backend: &FakeBackend) -> Vec<String> {
        backend
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("update_setting meetings"))
            .collect()
    }

    #[gpui_kit::test]
    fn user_emails_are_added_labelled_and_removed(cx: &mut TestAppContext) {
        use crate::logic::meetings::EmailError;
        use crate::ui::pages::meetings::emails::UserEmailsView;
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend.clone(), meetings);
        let (w, view) = open(cx, UserEmailsView::new);
        let enter = |cx: &mut TestAppContext, label: &str, email: &str| {
            let (label, email) = (label.to_string(), email.to_string());
            cx.update_window(w, |_, window, cx| {
                view.update(cx, |v, cx| v.enter(&label, &email, window, cx))
            })
            .unwrap();
            cx.run_until_parked();
        };
        enter(cx, "Personal", "me@home.example");
        enter(cx, " Work ", "me@work.example");
        let emails = backend.settings().meetings.user_emails;
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[1].label, "Work");
        assert!(exists(cx, w, "meetings-user-email-1"));
        // Bad and duplicate addresses are refused with a message.
        enter(cx, "", "ME@home.example");
        assert_eq!(
            view.read_with(cx, |v, _| v.error()),
            Some(EmailError::Duplicate)
        );
        assert!(exists(cx, w, "meetings-user-email-error"));
        enter(cx, "", "not-an-email");
        assert_eq!(
            view.read_with(cx, |v, _| v.error()),
            Some(EmailError::Invalid)
        );
        assert_eq!(backend.settings().meetings.user_emails.len(), 2);
        with_window(cx, w, |window, cx| {
            window.click("meetings-user-email-remove-0", cx)
        });
        let emails = backend.settings().meetings.user_emails;
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0].email, "me@work.example");
        assert!(!exists(cx, w, "meetings-user-email-1"));
    }

    #[gpui_kit::test]
    fn meetings_use_the_dictation_model_unless_another_is_chosen(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        let mut whisper = crate::logic::models::test_model("whisper");
        whisper.is_downloaded = true;
        backend.data.lock().models = vec![
            whisper,
            crate::logic::models::test_model(DEFAULT_MEETING_MODEL_ID),
        ];
        backend.with_settings(|s| {
            s.selected_model = "whisper".into();
            s.meetings.meeting_model = None;
        });
        setup_meetings(cx, backend.clone(), meetings);
        let (w, _) = open(cx, MeetingsPage::new);
        settings_tab(cx, w);
        // Parakeet isn't downloaded, but the dictation model is: no banner.
        assert!(!exists(cx, w, "meeting-model-missing-settings"));
        assert!(!exists(cx, w, "meeting-model-missing-list"));
        assert!(!exists(cx, w, "meetings-model-dropdown"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-model-separate-toggle", cx)
        });
        assert_eq!(
            backend.settings().meetings.meeting_model.as_deref(),
            Some(DEFAULT_MEETING_MODEL_ID)
        );
        assert!(exists(cx, w, "meetings-model-dropdown"));
        // The chosen model isn't downloaded: offered inline.
        assert!(exists(cx, w, "meeting-model-download-settings"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-model-separate-toggle", cx)
        });
        assert_eq!(backend.settings().meetings.meeting_model, None);
        assert!(!exists(cx, w, "meetings-model-dropdown"));
    }

    #[gpui_kit::test]
    fn meeting_settings_write_field_patches(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        backend.with_settings(|s| {
            s.meetings.enabled = false;
            s.meetings.user_display_name = "Imran".into();
        });
        meetings.data.lock().speaker_model = SpeakerModelStatus::Missing;
        let state = setup_meetings(cx, backend.clone(), meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        settings_tab(cx, w);
        with_window(cx, w, |window, cx| {
            window.click("meetings-enabled-toggle", cx)
        });
        assert_eq!(
            meetings_updates(&backend),
            vec![r#"update_setting meetings {"enabled":true}"#.to_string()]
        );
        // The speaker model downloads only from its button.
        assert!(!meetings.called("download_speaker_model"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-speaker-model-download", cx)
        });
        assert!(meetings.called("download_speaker_model"));
        // Another writer changed a different field meanwhile: a patch from
        // this view doesn't undo it.
        backend.with_settings(|s| s.meetings.user_display_name = "Someone".into());
        state.update(cx, |s, cx| s.refresh_settings(cx));
        cx.run_until_parked();
        with_window(cx, w, |window, cx| {
            window.click("meetings-calendar-notifications-toggle", cx)
        });
        let m = backend.settings().meetings;
        assert!(m.enabled);
        assert!(!m.calendar_notifications);
        assert_eq!(m.user_display_name, "Someone");
        assert_eq!(
            meetings_updates(&backend).last().unwrap(),
            r#"update_setting meetings {"calendar_notifications":false}"#
        );
    }

    #[gpui_kit::test]
    fn permissions_load_on_open_and_offer_fixes(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        backend.with_settings(|s| s.meetings.enabled = false);
        meetings.data.lock().permissions.calendar_granted = true;
        setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        settings_tab(cx, w);
        assert!(exists(cx, w, "meetings-permissions"));
        assert!(!exists(cx, w, "meetings-permission-open-calendar"));
        assert!(exists(cx, w, "meetings-permission-open-microphone"));
        // Notifications: once answered, macOS won't ask again, so the row
        // also links to the Notifications pane.
        assert!(exists(cx, w, "meetings-permission-open-notifications"));
        assert!(exists(cx, w, "meetings-permission-request-notifications"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-permission-open-system-audio", cx)
        });
        assert!(meetings.called(
            "open_url x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
        ));
        with_window(cx, w, |window, cx| {
            window.click("meetings-permission-request-microphone", cx)
        });
        assert!(meetings.called("request_permissions"));
    }

    #[gpui_kit::test]
    fn speaker_model_row_follows_status_events(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().speaker_model = SpeakerModelStatus::Missing;
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        settings_tab(cx, w);
        assert!(exists(cx, w, "meetings-speaker-model"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-speaker-model-download", cx)
        });
        assert!(meetings.called("download_speaker_model"));
        let st = SpeakerModelStatus::Downloading {
            downloaded: 10,
            total: 20,
        };
        dispatch(
            cx,
            &state,
            "meetings://speaker-model-status",
            serde_json::to_value(&st).unwrap(),
        );
        assert!(exists(cx, w, "meetings-speaker-model-progress"));
        dispatch(
            cx,
            &state,
            "meetings://speaker-model-status",
            serde_json::to_value(SpeakerModelStatus::Ready).unwrap(),
        );
        assert!(!exists(cx, w, "meetings-speaker-model-progress"));
        assert!(!exists(cx, w, "meetings-speaker-model-download"));
    }

    #[gpui_kit::test]
    fn init_failure_shows_retry_and_ready_reloads(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().init = MeetingsInit::Failed("Keychain denied".into());
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        assert!(exists(cx, w, "meetings-init-failed"));
        assert!(!exists(cx, w, "meeting-1"));
        // Not a key problem: the notes may be readable, so no reset.
        assert!(exists(cx, w, "meetings-init-retry"));
        assert!(!exists(cx, w, "meetings-init-reset"));
        // Retry while the manager still can't start: still failed.
        with_window(cx, w, |window, cx| window.click("meetings-init-retry", cx));
        assert!(meetings.called("retry_init"));
        assert!(exists(cx, w, "meetings-init-failed"));
        // A start in progress, then the ready event.
        meetings.data.lock().init = MeetingsInit::Starting;
        with_window(cx, w, |window, cx| window.click("meetings-init-retry", cx));
        assert!(exists(cx, w, "meetings-starting"));
        meetings
            .data
            .lock()
            .meetings
            .push(meeting_fixture(3, "Late", MeetingStatus::Recorded));
        dispatch(cx, &state, "meetings://ready", serde_json::Value::Null);
        assert_eq!(
            state.read_with(cx, |s, _| s.meetings_init.clone()),
            MeetingsInit::Ready
        );
        assert!(exists(cx, w, "meeting-3"));
        dispatch(
            cx,
            &state,
            "meetings://init-failed",
            serde_json::json!("store is corrupt"),
        );
        assert!(exists(cx, w, "meetings-init-failed"));
    }

    #[gpui_kit::test]
    async fn missing_data_key_offers_a_confirmed_reset(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().init = MeetingsInit::NotStarted;
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        // Never used: no banner.
        assert!(!exists(cx, w, "meetings-init-failed"));
        assert!(!exists(cx, w, "meetings-init-key-missing"));
        dispatch(
            cx,
            &state,
            guidy_core::MEETINGS_DATA_KEY_MISSING_EVENT,
            serde_json::json!("key gone"),
        );
        assert!(exists(cx, w, "meetings-init-key-missing"));
        assert!(exists(cx, w, "meetings-init-retry"));
        with_window(cx, w, |window, cx| window.click("meetings-init-reset", cx));
        // Nothing happens until the dialog is confirmed.
        assert!(!meetings.called("reset_data"));
        cx.update_window(w, |_, window, cx| window.press("enter", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(meetings.called("reset_data"));
        assert_eq!(
            state.read_with(cx, |s, _| s.meetings_init.clone()),
            MeetingsInit::Ready
        );
        assert!(!exists(cx, w, "meetings-init-key-missing"));
    }

    #[gpui_kit::test]
    fn missing_meeting_model_offers_a_download(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        backend.data.lock().models =
            vec![crate::logic::models::test_model(DEFAULT_MEETING_MODEL_ID)];
        let state = setup_meetings(cx, backend.clone(), meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        assert!(exists(cx, w, "meeting-model-missing-list"));
        with_window(cx, w, |window, cx| {
            window.click("meeting-model-download-list", cx)
        });
        assert!(backend.called(&format!("download_model {DEFAULT_MEETING_MODEL_ID}")));
        assert!(state.read_with(cx, |s, _| s
            .meeting_model_downloads
            .contains(DEFAULT_MEETING_MODEL_ID)));
        // While downloading the banner shows progress instead.
        assert!(!exists(cx, w, "meeting-model-download-list"));
        // Finished: the dictation model is left alone.
        backend.with_settings(|_| {});
        backend.data.lock().models[0].is_downloaded = true;
        dispatch(
            cx,
            &state,
            "model-download-complete",
            serde_json::json!(DEFAULT_MEETING_MODEL_ID),
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        cx.run_until_parked();
        assert!(!backend.called("set_active_model"));
        assert!(!exists(cx, w, "meeting-model-missing-list"));
        // A recorded meeting's detail and the settings show it too.
        backend.data.lock().models[0].is_downloaded = false;
        state.update(cx, |s, cx| s.load_models(cx));
        cx.run_until_parked();
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| p.open_meeting(2, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert!(exists(cx, w, "meeting-model-download-detail"));
        settings_tab(cx, w);
        assert!(exists(cx, w, "meeting-model-download-settings"));
    }

    #[gpui_kit::test]
    fn meeting_model_download_finishing_during_onboarding_is_cleaned_up(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        backend.data.lock().models =
            vec![crate::logic::models::test_model(DEFAULT_MEETING_MODEL_ID)];
        let state = setup_meetings(cx, backend.clone(), meetings);
        state.update(cx, |s, cx| {
            s.download_meeting_model(DEFAULT_MEETING_MODEL_ID, cx)
        });
        cx.run_until_parked();
        assert!(state.read_with(cx, |s, _| s
            .meeting_model_downloads
            .contains(DEFAULT_MEETING_MODEL_ID)));
        // Onboarding (re)starts while the download runs.
        state.update(cx, |s, _| {
            s.startup.step = Some(crate::logic::flows::OnboardingStep::Model)
        });
        backend.data.lock().models[0].is_downloaded = true;
        dispatch(
            cx,
            &state,
            "model-download-complete",
            serde_json::json!(DEFAULT_MEETING_MODEL_ID),
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        cx.run_until_parked();
        assert!(state.read_with(cx, |s, _| s.meeting_model_downloads.is_empty()));
        assert!(!backend.called("set_active_model"));
    }

    #[gpui_kit::test]
    fn processing_with_a_missing_model_is_a_typed_error(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().fail_process = Some(
            MeetingsError::ModelMissing {
                model_id: "m".into(),
                name: "Parakeet".into(),
                known: true,
            }
            .encode(),
        );
        setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-process-2", cx));
        assert!(meetings.called("process_meeting 2"));
        // The status stays "recorded" (nothing was queued).
        assert!(exists(cx, w, "meeting-process-2"));
    }

    #[gpui_kit::test]
    fn detail_cancels_processing(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().meetings[1].status = MeetingStatus::Processing;
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| p.open_meeting(2, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        with_window(cx, w, |window, cx| {
            window.click("meeting-detail-cancel", cx)
        });
        assert!(meetings.called("cancel_processing 2"));
    }

    #[gpui_kit::test]
    fn summary_reloads_and_shows_progress_with_cancel(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        {
            let mut d = meetings.data.lock();
            d.ai.summaries.insert(
                1,
                StoredSummary {
                    meeting_id: 1,
                    title: "Old".into(),
                    short_summary: String::new(),
                    decisions: vec![],
                    topics: vec![],
                    generated_by: "x".into(),
                    generated_at: 1,
                    usage: None,
                },
            );
            d.ai.items = vec![action_item_fixture(7, 1, "send notes")];
        }
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, view) = open(cx, |window, cx| SummaryView::new(1, window, cx));
        with_window(cx, w, |window, cx| window.click("action-items-fold", cx));
        assert!(exists(cx, w, "action-item-7"));
        assert!(!exists(cx, w, "action-item-8"));
        // Changed elsewhere (e.g. regenerated in another window).
        meetings
            .data
            .lock()
            .ai
            .items
            .push(action_item_fixture(8, 1, "book a room"));
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(1));
        assert!(exists(cx, w, "action-item-8"));
        // Another meeting's change doesn't matter.
        meetings.data.lock().ai.items.clear();
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(2));
        assert!(exists(cx, w, "action-item-8"));

        // A summary running elsewhere: progress + Cancel.
        meetings.data.lock().ai.summarizing = vec![1];
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(1));
        assert!(view.read_with(cx, |v, _| v.is_generating()));
        dispatch(
            cx,
            &state,
            "meetings://summary-progress",
            serde_json::json!({"meeting_id": 1, "done": 1, "total": 3}),
        );
        assert!(exists(cx, w, "summary-progress"));
        with_window(cx, w, |window, cx| {
            window.click("summary-cancel-generate", cx)
        });
        assert!(meetings.called("cancel_summarize 1"));
        meetings.data.lock().ai.summarizing.clear();
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(1));
        assert!(!exists(cx, w, "summary-progress"));
    }

    #[gpui_kit::test]
    fn refused_cancel_keeps_the_job_and_shows_its_error(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().ai.cancel_refused = true;
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, view) = open(cx, |window, cx| SummaryView::new(1, window, cx));
        cx.run_until_parked();

        // A job running elsewhere is already storing: Cancel is refused and
        // the spinner stays until the job ends.
        meetings.data.lock().ai.summarizing = vec![1];
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(1));
        dispatch(
            cx,
            &state,
            "meetings://summary-progress",
            serde_json::json!({"meeting_id": 1, "done": 2, "total": 3}),
        );
        with_window(cx, w, |window, cx| {
            window.click("summary-cancel-generate", cx)
        });
        assert!(meetings.called("cancel_summarize 1"));
        assert!(view.read_with(cx, |v, _| v.is_generating()));
        assert!(exists(cx, w, "summary-progress"));
        meetings.data.lock().ai.summarizing.clear();
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(1));
        assert!(!view.read_with(cx, |v, _| v.is_generating()));

        // This view's own job: still generating after the refusal, and its
        // store error is shown rather than taken for the cancel.
        meetings.data.lock().ai.fail_summarize = Some("could not store".into());
        view.update(cx, |v, cx| {
            v.generate(None, cx);
            v.cancel(cx);
            v.finish_cancel(false, cx);
            assert!(v.is_generating());
        });
        cx.run_until_parked();
        assert!(!view.read_with(cx, |v, _| v.is_generating()));
        assert!(exists(cx, w, "summary-error"));
    }

    #[gpui_kit::test]
    fn cancel_hides_only_the_jobs_cancelled_error(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, view) = open(cx, |window, cx| SummaryView::new(1, window, cx));
        cx.run_until_parked();

        // The job ends with its own error while Cancel is still pending (the
        // summarize result arrives first): the error is shown.
        meetings.data.lock().ai.fail_summarize = Some("could not store".into());
        view.update(cx, |v, cx| {
            v.generate(None, cx);
            v.cancel(cx);
        });
        cx.run_until_parked();
        assert!(meetings.called("cancel_summarize 1"));
        assert!(!view.read_with(cx, |v, _| v.is_generating()));
        assert!(exists(cx, w, "summary-error"));

        // The cancel stopped the job: its "cancelled" error isn't shown.
        meetings.data.lock().ai.fail_summarize = Some(MeetingsError::Cancelled.encode());
        view.update(cx, |v, cx| {
            v.generate(None, cx);
            v.cancel(cx);
        });
        cx.run_until_parked();
        assert!(!view.read_with(cx, |v, _| v.is_generating()));
        assert!(!exists(cx, w, "summary-error"));
    }

    #[gpui_kit::test]
    fn summary_job_state_follows_the_shown_meeting(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, view) = open(cx, |window, cx| SummaryView::new(1, window, cx));
        cx.run_until_parked();

        // Meeting 1 summarizes (here), meeting 2 is summarizing elsewhere.
        meetings.data.lock().ai.summarizing = vec![2];
        view.update(cx, |v, cx| {
            v.generate(None, cx);
            assert!(v.is_generating());
            v.set_meeting(2, cx);
            // Meeting 1's spinner doesn't carry over.
            assert!(!v.is_generating());
        });
        cx.run_until_parked();
        assert!(meetings.called("summarize 1 default"));
        // Meeting 2's own job shows; meeting 1's finishing doesn't clear it
        // or show its summary here.
        assert!(view.read_with(cx, |v, _| v.is_generating()));
        assert!(exists(cx, w, "summary-progress"));
        assert!(exists(cx, w, "summary-empty"));
        with_window(cx, w, |window, cx| {
            window.click("summary-cancel-generate", cx)
        });
        assert!(meetings.called("cancel_summarize 2"));
        assert!(!meetings.called("cancel_summarize 1"));

        // Back to meeting 1: its finished summary loads, no spinner.
        meetings.data.lock().ai.summarizing.clear();
        view.update(cx, |v, cx| v.set_meeting(1, cx));
        cx.run_until_parked();
        assert!(!view.read_with(cx, |v, _| v.is_generating()));
        assert!(!exists(cx, w, "summary-empty"));
    }

    /// Hosts the ticket list through its public entry point.
    struct TicketsHost;
    impl gpui_kit::Render for TicketsHost {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::{ParentElement, Styled};
            gpui_kit::div()
                .size_full()
                .child(crate::ui::pages::meetings::tickets::render(1, window, cx))
        }
    }

    #[gpui_kit::test]
    fn tickets_refresh_on_key_and_meeting_changes(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        let mut drafted = action_item_fixture(31, 1, "write docs");
        drafted.ticket.title = "Docs".into();
        drafted.ticket.team_id = Some("team-1".into());
        meetings.data.lock().ai.items = vec![drafted];
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, |_, _| TicketsHost);
        assert!(exists(cx, w, "tickets-no-key"));
        // The key is saved in AI settings.
        meetings.data.lock().ai.linear_key = Some("lin_api_1".into());
        state.update(cx, |s, cx| {
            s.emit_meetings(MeetingsEvent::LinearKeyChanged, cx)
        });
        cx.run_until_parked();
        assert!(!exists(cx, w, "tickets-no-key"));
        assert!(!exists(cx, w, "ticket-approve-31"), "folded");
        with_window(cx, w, |window, cx| window.click("tickets-fold", cx));
        assert!(exists(cx, w, "ticket-approve-31"));
        // A created ticket arrives through a meeting change.
        {
            let mut d = meetings.data.lock();
            let i = &mut d.ai.items[0];
            i.ticket_state = guidy_core::meetings::types::TicketState::Created;
            i.linear_issue_id = Some("issue".into());
            i.linear_issue_identifier = Some("ENG-31".into());
            i.linear_issue_url = Some("https://evil.example/ENG-31".into());
        }
        dispatch(cx, &state, events::MEETING_CHANGED, serde_json::json!(1));
        assert!(exists(cx, w, "ticket-issue-31"));
        // Not a linear.app link: never opened.
        with_window(cx, w, |window, cx| window.click("ticket-issue-31", cx));
        assert!(!meetings.called("open_url"), "{:?}", meetings.calls());
    }

    #[gpui_kit::test]
    fn saving_a_linear_key_refreshes_open_ticket_lists(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        let mut drafted = action_item_fixture(41, 1, "x");
        drafted.ticket.title = "X".into();
        meetings.data.lock().ai.items = vec![drafted];
        setup_meetings(cx, backend, meetings.clone());
        let (tw, _) = open(cx, |_, _| TicketsHost);
        assert!(exists(cx, tw, "tickets-no-key"));
        let (w, view) = open(cx, AiSettingsView::new);
        cx.update_window(w, |_, window, cx| {
            view.update(cx, |v, cx| v.enter_linear_key("lin_api_9", window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        assert!(!exists(cx, tw, "tickets-no-key"));
    }

    #[gpui_kit::test]
    fn local_model_events_update_ai_settings(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().ai.local_models = vec![crate::meetings_backend::LocalLlmModel {
            id: "qwen3-4b".into(),
            name: "Qwen3 4B".into(),
            description: String::new(),
            size_mb: 2500,
            is_default: true,
            recommended_ram_gb: None,
            fits_this_mac: true,
            downloaded: false,
            download_progress: None,
        }];
        let state = setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, AiSettingsView::new);
        assert!(exists(cx, w, "local-download-qwen3-4b"));
        dispatch(
            cx,
            &state,
            "llm-models://download-progress",
            serde_json::json!({"model_id": "qwen3-4b", "downloaded": 10, "total": 100, "percentage": 10.0}),
        );
        assert!(exists(cx, w, "local-progress-qwen3-4b"));
        meetings.data.lock().ai.local_models[0].downloaded = true;
        dispatch(
            cx,
            &state,
            "llm-models://changed",
            serde_json::json!({"model_id": "qwen3-4b", "state": "completed", "error": null}),
        );
        assert!(!exists(cx, w, "local-progress-qwen3-4b"));
        assert!(exists(cx, w, "local-delete-qwen3-4b"));
    }

    #[gpui_kit::test]
    fn deep_link_opens_the_meeting_and_plays_at_t(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        let link = crate::logic::meetings::parse_deep_link("guidy://meetings/1?t=61000").unwrap();
        assert_eq!(
            link,
            MeetingLink {
                meeting_id: 1,
                t_ms: Some(61_000)
            }
        );
        main.update(cx, |m, cx| m.open_meeting_link(link, cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "meeting-detail"));
        cx.run_until_parked();
        assert_eq!(main.read_with(cx, |m, _| m.section), Section::Meetings);
        // The segment containing 61 s plays.
        assert!(
            meetings.called("segment_audio 1 system 60000 62000"),
            "{:?}",
            meetings.calls()
        );
    }

    #[gpui_kit::test]
    fn deep_links_are_ignored_without_meeting_notes(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().supported = false;
        setup_meetings(cx, backend, meetings);
        let (_w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        let link = MeetingLink {
            meeting_id: 1,
            t_ms: None,
        };
        main.update(cx, |m, cx| m.open_meeting_link(link, cx));
        cx.run_until_parked();
        assert_ne!(main.read_with(cx, |m, _| m.section), Section::Meetings);
    }

    #[gpui_kit::test]
    fn post_processing_reads_the_key_off_the_render_path(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        setup(cx, backend.clone());
        let (_w, page) = open(cx, PostProcessingPage::new);
        cx.run_until_parked();
        let pid = backend.settings().post_process_provider_id.clone();
        let pid = if pid.is_empty() {
            backend.settings().post_process_providers[0].id.clone()
        } else {
            pid
        };
        assert!(page.read_with(cx, |p, _| p.cached_api_key(&pid).is_some()));
    }

    #[gpui_kit::test]
    fn failed_api_key_save_keeps_the_cached_key(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, page) = open(cx, PostProcessingPage::new);
        cx.run_until_parked();
        let pid = backend.settings().post_process_provider_id.clone();
        let pid = if pid.is_empty() {
            backend.settings().post_process_providers[0].id.clone()
        } else {
            pid
        };
        let old = page
            .read_with(cx, |p, _| p.cached_api_key(&pid).map(str::to_string))
            .expect("key loaded");
        let toasts = std::rc::Rc::new(std::cell::RefCell::new(0));
        let t2 = toasts.clone();
        let _sub = cx.update(|cx| {
            cx.subscribe(&state, move |_, ev: &crate::state::AppEvent, _| {
                if matches!(ev, crate::state::AppEvent::Toast(_)) {
                    *t2.borrow_mut() += 1;
                }
            })
        });

        // Failure: the cache keeps the old key, the input shows it again.
        backend.data.lock().fail_settings = vec!["post_process_api_key".into()];
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| p.enter_api_key("sk-new", window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        assert!(backend.called(&format!("change_post_process_api_key {pid} sk-new")));
        assert_eq!(
            page.read_with(cx, |p, _| p.cached_api_key(&pid).map(str::to_string)),
            Some(old.clone())
        );
        assert_eq!(page.read_with(cx, |p, cx| p.api_key_input(cx)), old);
        assert_eq!(*toasts.borrow(), 1);

        // Success: cached once the backend confirms.
        backend.data.lock().fail_settings.clear();
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| p.enter_api_key("sk-good", window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            page.read_with(cx, |p, _| p.cached_api_key(&pid).map(str::to_string)),
            Some("sk-good".into())
        );
        assert_eq!(*toasts.borrow(), 1);
    }

    #[gpui_kit::test]
    fn api_key_typed_before_the_stored_key_loads_is_saved(cx: &mut TestAppContext) {
        let backend = FakeBackend::new();
        let state = setup(cx, backend.clone());
        let (w, page) = open(cx, PostProcessingPage::new);
        let shown = backend.settings().post_process_provider_id.clone();
        let shown = if shown.is_empty() {
            backend.settings().post_process_providers[0].id.clone()
        } else {
            shown
        };
        let pid = backend
            .settings()
            .post_process_providers
            .iter()
            .map(|p| p.id.clone())
            .find(|id| *id != shown && id != "apple_intelligence" && id != "guidy_local")
            .expect("another BYOK provider");
        // Showing another provider starts its Keychain read; the key is
        // typed and saved before that read finishes.
        cx.update_window(w, |_, window, cx| {
            state.update(cx, |s, cx| s.set_post_process_provider(&pid, cx));
            assert!(page.read_with(cx, |p, _| p.cached_api_key(&pid).is_none()));
            page.update(cx, |p, cx| p.enter_api_key("sk-early", window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        assert!(backend.called(&format!("change_post_process_api_key {pid} sk-early")));
        // The finished read doesn't bring back the old (empty) key.
        assert_eq!(
            page.read_with(cx, |p, _| p.cached_api_key(&pid).map(str::to_string)),
            Some("sk-early".into())
        );
        assert_eq!(page.read_with(cx, |p, cx| p.api_key_input(cx)), "sk-early");
    }

    #[gpui_kit::test]
    fn post_processing_offers_the_builtin_model(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        let state = setup_meetings(cx, backend.clone(), meetings);
        let (w, _) = open(cx, PostProcessingPage::new);
        assert!(exists(cx, w, "pp-api-key"));
        assert!(!exists(cx, w, "pp-builtin"));
        state.update(cx, |s, cx| s.set_post_process_provider("guidy_local", cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "pp-builtin"));
        assert!(exists(cx, w, "pp-builtin-manage"));
        assert!(!exists(cx, w, "pp-api-key"));
        assert!(!exists(cx, w, "pp-base-url"));
        assert!(exists(cx, w, "pp-model"));
    }

    #[gpui_kit::test]
    fn builtin_manage_opens_meetings_settings(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings);
        let (w, main) = open(cx, MainWindow::new);
        cx.run_until_parked();
        main.update(cx, |m, cx| m.open_meetings_settings(cx));
        cx.run_until_parked();
        assert_eq!(main.read_with(cx, |m, _| m.section), Section::Meetings);
        assert!(exists(cx, w, "meetings-settings"));
    }

    #[gpui_kit::test]
    fn pp_test_connection_saves_typed_key_and_base_url_first(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        // The backend tests what is stored: fail unless both were saved.
        let b = backend.clone();
        meetings.data.lock().ai.llm_test = Some(Arc::new(move |pid| {
            let key = b.called(&format!("change_post_process_api_key {pid} sk-typed"));
            let url = b.called(&format!(
                "change_post_process_base_url {pid} https://typed.example/v1"
            ));
            match (key, url) {
                (true, true) => Ok("Connected".into()),
                (false, _) => Err("no API key set".into()),
                (_, false) => Err("old base URL".into()),
            }
        }));
        let state = setup_meetings(cx, backend.clone(), meetings.clone());
        state.update(cx, |s, cx| s.set_post_process_provider("custom", cx));
        cx.run_until_parked();
        let (w, page) = open(cx, PostProcessingPage::new);
        // Typed, never blurred: the test saves them, then runs.
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| {
                p.type_base_url("https://typed.example/v1", window, cx);
                p.type_api_key("sk-typed", window, cx);
            })
        })
        .unwrap();
        with_window(cx, w, |window, cx| {
            window.click("pp-test-connection-button", cx)
        });
        assert!(meetings.called("llm_test_connection custom PostProcess"));
        assert_eq!(
            page.read_with(cx, |p, _| p.test_outcome().cloned()),
            Some(Ok("Connected".into()))
        );
    }

    #[gpui_kit::test]
    fn pp_test_connection_waits_for_an_in_flight_key_save(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        let b = backend.clone();
        meetings.data.lock().ai.llm_test = Some(Arc::new(move |pid| {
            if b.called(&format!("change_post_process_api_key {pid} sk-blurred")) {
                Ok("Connected".into())
            } else {
                Err("no API key set".into())
            }
        }));
        let state = setup_meetings(cx, backend.clone(), meetings.clone());
        state.update(cx, |s, cx| s.set_post_process_provider("custom", cx));
        cx.run_until_parked();
        let (w, page) = open(cx, PostProcessingPage::new);
        // The blur's save is still in flight when the test is clicked.
        cx.update_window(w, |_, window, cx| {
            page.update(cx, |p, cx| p.enter_api_key("sk-blurred", window, cx));
            window.render_frame(cx);
            window.click("pp-test-connection-button", cx);
            // Chained after the save, not racing it.
            assert!(page.read(cx).test_waiting());
            assert!(page.read(cx).saving_api_key("custom"));
        })
        .unwrap();
        cx.run_until_parked();
        assert!(!page.read_with(cx, |p, _| p.test_waiting()));
        assert_eq!(
            page.read_with(cx, |p, _| p.test_outcome().cloned()),
            Some(Ok("Connected".into()))
        );
    }

    #[gpui_kit::test]
    fn builtin_models_are_managed_from_post_processing_without_meetings(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        {
            let mut d = meetings.data.lock();
            d.supported = false;
            d.ai.local_models = vec![crate::meetings_backend::LocalLlmModel {
                id: "qwen3-4b".into(),
                name: "Qwen3 4B".into(),
                description: String::new(),
                size_mb: 2500,
                is_default: true,
                recommended_ram_gb: None,
                fits_this_mac: true,
                downloaded: false,
                download_progress: None,
            }];
        }
        let state = setup_meetings(cx, backend.clone(), meetings.clone());
        assert!(!state.read_with(cx, |s, _| s.meetings_supported));
        let (w, _) = open(cx, PostProcessingPage::new);
        state.update(cx, |s, cx| s.set_post_process_provider("guidy_local", cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "pp-builtin-manage"));
        assert!(!exists(cx, w, "local-models"));
        with_window(cx, w, |window, cx| window.click("pp-builtin-manage", cx));
        assert!(exists(cx, w, "local-download-qwen3-4b"));
        with_window(cx, w, |window, cx| {
            window.click("local-download-qwen3-4b", cx)
        });
        assert!(meetings.called("download_local_llm_model qwen3-4b"));
        // A finished download refreshes the built-in model choices.
        let fetches = || {
            backend
                .calls()
                .iter()
                .filter(|c| *c == "fetch_post_process_models guidy_local")
                .count()
        };
        let before = fetches();
        meetings.data.lock().ai.local_models[0].downloaded = true;
        dispatch(cx, &state, "llm-models://changed", serde_json::json!(null));
        assert_eq!(fetches(), before + 1);
        assert!(exists(cx, w, "local-delete-qwen3-4b"));
        // Hidden again for another provider.
        state.update(cx, |s, cx| s.set_post_process_provider("custom", cx));
        cx.run_until_parked();
        assert!(!exists(cx, w, "local-models"));
    }
}

mod recording_widget {
    use super::*;
    use crate::testing::FakeMeetingsBackend;
    use crate::ui::recording_widget::{RecordingWidgetState, RecordingWidgetView};
    use guidy_core::meetings::types::{LiveLine, LiveStatus, LiveUpdate, RecorderState, Track};

    fn recording(meeting_id: i64) -> RecorderState {
        RecorderState::Recording {
            meeting_id,
            title: "Weekly sync".into(),
            started_at: 0,
            scheduled_end_at: None,
            paused_at: None,
            paused_ms: 0,
        }
    }

    fn line(id: u64, track: Track, text: &str, tentative: bool) -> LiveUpdate {
        LiveUpdate {
            meeting_id: 4,
            status: Some(LiveStatus::Live),
            line: Some(LiveLine {
                id,
                track,
                t0_ms: id as i64 * 1_000,
                t1_ms: id as i64 * 1_000 + 900,
                text: text.into(),
                tentative,
            }),
        }
    }

    /// The widget on a display the size of the test window, not animated.
    fn widget_setup(
        cx: &mut TestAppContext,
        docked: bool,
    ) -> (
        Arc<FakeMeetingsBackend>,
        Entity<RecordingWidgetState>,
        AnyWindowHandle,
    ) {
        let meetings = FakeMeetingsBackend::new();
        setup_meetings(cx, FakeBackend::new(), meetings.clone());
        let widget = cx.update(RecordingWidgetState::install);
        cx.update(|cx| {
            widget.update(cx, |s, _| {
                s.animate = false;
                s.docked = docked;
                s.desk =
                    crate::logic::floater::Desk::single(crate::logic::floater::Stage::from_screen(
                        &crate::logic::notch::ScreenInfo {
                            x: 0.0,
                            y: 0.0,
                            width: 900.0,
                            height: 1000.0,
                            notch: Some((380.0, 520.0)),
                            top_inset: 38.0,
                        },
                        crate::ui::design::SCALE,
                    ));
            })
        });
        let (w, _) = open(cx, |window, cx| RecordingWidgetView::new(0, window, cx));
        (meetings, widget, w)
    }

    #[gpui_kit::test]
    fn the_widget_follows_the_recording_and_its_controls_reach_the_backend(
        cx: &mut TestAppContext,
    ) {
        let (meetings, widget, w) = widget_setup(cx, false);
        assert!(!exists(cx, w, "recording-widget-blob"), "hidden while idle");

        cx.update(|cx| {
            widget.update(cx, |s, cx| {
                s.apply_live(&line(0, Track::System, "Can you see my screen?", false), cx);
                s.set_recorder(recording(4), cx);
            })
        });
        // Free: the squircle with its stop button, no wings.
        assert!(exists(cx, w, "recording-widget-blob"));
        assert!(exists(cx, w, "recording-widget-stop-blob"));
        assert!(!exists(cx, w, "recording-widget-wings"));
        assert!(!exists(cx, w, "recording-widget-panel"));

        // A click on the squircle opens the panel.
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-blob", cx)
        });
        assert!(widget.read_with(cx, |s, _| s.model.expanded));
        cx.update(|cx| {
            widget.update(cx, |s, cx| {
                s.apply_live(&line(1, Track::Mic, "Yes, go ahead", true), cx);
            })
        });
        assert!(exists(cx, w, "recording-widget-panel"));
        assert!(!exists(cx, w, "recording-widget-blob"));
        assert!(exists(cx, w, "recording-widget-title"));
        assert!(exists(cx, w, "source-mic"));
        assert!(exists(cx, w, "source-system"));
        assert!(exists(cx, w, "live-row-0"));
        assert!(exists(cx, w, "live-row-1"));

        with_window(cx, w, |window, cx| {
            window.click("recording-widget-pause", cx)
        });
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-stop", cx)
        });
        let calls = meetings.calls();
        assert!(
            calls.contains(&"set_recording_paused true".to_string()),
            "{calls:?}"
        );
        assert!(calls.contains(&"stop_recording".to_string()), "{calls:?}");

        // Collapsed again: back to the floater, whose pause and stop work too.
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-collapse", cx)
        });
        assert!(exists(cx, w, "recording-widget-blob"));
        with_window(cx, w, |window, cx| {
            window.hover("blob-wave", cx);
            window.click("recording-widget-pause-blob", cx)
        });
        let pauses = meetings
            .calls()
            .iter()
            .filter(|c| *c == "set_recording_paused true")
            .count();
        assert_eq!(pauses, 2);
        assert!(!widget.read_with(cx, |s, _| s.model.expanded));
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-stop-blob", cx)
        });
        let stops = meetings
            .calls()
            .iter()
            .filter(|c| *c == "stop_recording")
            .count();
        assert_eq!(stops, 2);
        assert!(
            !widget.read_with(cx, |s, _| s.model.expanded),
            "the stop button doesn't open the panel"
        );

        cx.update(|cx| widget.update(cx, |s, cx| s.set_recorder(RecorderState::Idle, cx)));
        assert!(
            !exists(cx, w, "recording-widget-blob"),
            "gone after the stop"
        );
    }

    #[gpui_kit::test]
    fn docked_it_wraps_the_notch_and_grows_into_the_panel(cx: &mut TestAppContext) {
        let (meetings, widget, w) = widget_setup(cx, true);
        cx.update(|cx| widget.update(cx, |s, cx| s.set_recorder(recording(4), cx)));
        assert!(exists(cx, w, "recording-widget-wings"));
        assert!(exists(cx, w, "recording-widget-stop-wings"));
        assert!(!exists(cx, w, "recording-widget-blob"));

        // Pause over the waveform (shown on hover); it doesn't open the panel.
        let visible = |cx: &mut TestAppContext| {
            cx.update_window(w, |_, window, cx| {
                window.render_frame(cx);
                window.find("recording-widget-pause-wings").visible()
            })
            .unwrap()
        };
        assert!(!visible(cx), "hidden until hovered");
        with_window(cx, w, |window, cx| window.hover("wings-wave", cx));
        assert!(visible(cx), "shown on hover");
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-pause-wings", cx)
        });
        assert!(meetings.called("set_recording_paused true"));
        assert!(!widget.read_with(cx, |s, _| s.model.expanded));

        with_window(cx, w, |window, cx| {
            window.click("recording-widget-stop-wings", cx)
        });
        assert!(meetings.called("stop_recording"));
        assert!(!widget.read_with(cx, |s, _| s.model.expanded));

        // A click on the wings: the notch grows into the panel.
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-wings", cx)
        });
        assert!(widget.read_with(cx, |s, _| s.model.expanded));
        assert!(exists(cx, w, "recording-widget-panel"));
        assert!(!exists(cx, w, "recording-widget-stop-wings"));
        // Its timer and collapse sit beside the notch, not in a header row.
        assert!(exists(cx, w, "recording-widget-collapse"));

        // The setting changes mid-recording: it floats off.
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-collapse", cx)
        });
        cx.update(|cx| {
            widget.update(cx, |s, cx| {
                s.docked = false;
                cx.notify();
            })
        });
        assert!(exists(cx, w, "recording-widget-blob"));
        assert!(!exists(cx, w, "recording-widget-wings"));
    }

    #[gpui_kit::test]
    fn dragged_to_the_notch_it_docks_and_pulled_away_it_floats(cx: &mut TestAppContext) {
        let (_, widget, w) = widget_setup(cx, false);
        cx.update(|cx| widget.update(cx, |s, cx| s.set_recorder(recording(4), cx)));
        let center = |cx: &mut TestAppContext, id: &'static str| {
            cx.update_window(w, |_, window, cx| {
                window.render_frame(cx);
                window.find(id).bounds().center()
            })
            .unwrap()
        };
        // Into the notch (380..520 on this display): docked, remembered.
        let from = center(cx, "recording-widget-blob");
        with_window(cx, w, |window, cx| {
            window.drag(from, gpui_kit::point(px(450.), px(60.)), cx)
        });
        assert!(exists(cx, w, "recording-widget-wings"));
        assert!(!exists(cx, w, "recording-widget-blob"));
        assert!(widget.read_with(cx, |s, _| s.docked));
        assert!(
            !widget.read_with(cx, |s, _| s.model.expanded),
            "a drag isn't a click"
        );

        // Pulled well away: free again, where it was let go.
        with_window(cx, w, |window, cx| {
            window.drag(
                gpui_kit::point(px(300.), px(19.)),
                gpui_kit::point(px(300.), px(600.)),
                cx,
            )
        });
        assert!(exists(cx, w, "recording-widget-blob"));
        assert!(!exists(cx, w, "recording-widget-wings"));
        assert!(!widget.read_with(cx, |s, _| s.docked));
        let at = center(cx, "recording-widget-blob");
        assert!(
            (at.x - px(300.)).abs() < px(2.) && (at.y - px(600.)).abs() < px(2.),
            "{at:?}"
        );
    }

    #[gpui_kit::test]
    fn dragged_into_a_corner_it_becomes_a_leaf(cx: &mut TestAppContext) {
        let (meetings, widget, w) = widget_setup(cx, false);
        cx.update(|cx| widget.update(cx, |s, cx| s.set_recorder(recording(4), cx)));
        let from = cx
            .update_window(w, |_, window, cx| {
                window.render_frame(cx);
                window.find("recording-widget-blob").bounds().center()
            })
            .unwrap();
        // The bottom-right corner of this 900x1000 display.
        with_window(cx, w, |window, cx| {
            window.drag(from, gpui_kit::point(px(885.), px(985.)), cx)
        });
        assert!(exists(cx, w, "recording-widget-leaf"));
        assert!(!exists(cx, w, "recording-widget-blob"));
        assert!(
            !widget.read_with(cx, |s, _| s.docked),
            "a corner isn't the notch setting"
        );

        // A click on its waveform pauses; its stop button stops.
        with_window(cx, w, |window, cx| window.click("leaf-wave", cx));
        assert!(meetings.called("set_recording_paused true"));
        assert!(!widget.read_with(cx, |s, _| s.model.expanded));
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-stop-leaf", cx)
        });
        assert!(meetings.called("stop_recording"));

        // A click on the leaf opens the panel beside it.
        let leaf = cx
            .update_window(w, |_, window, cx| {
                window.render_frame(cx);
                window.find("recording-widget-leaf").bounds()
            })
            .unwrap();
        with_window(cx, w, |window, cx| {
            window.click_at("recording-widget-leaf", gpui_kit::point(px(3.), px(3.)), cx)
        });
        assert!(widget.read_with(cx, |s, _| s.model.expanded));
        assert!(exists(cx, w, "recording-widget-panel"));
        assert!(exists(cx, w, "recording-widget-leaf"));
        with_window(cx, w, |window, cx| {
            window.click("recording-widget-collapse", cx)
        });

        // Pulled out toward the middle: free again.
        let grab = leaf.origin + gpui_kit::point(px(3.), px(3.));
        with_window(cx, w, |window, cx| {
            window.drag(grab, gpui_kit::point(px(400.), px(500.)), cx)
        });
        assert!(exists(cx, w, "recording-widget-blob"));
        assert!(!exists(cx, w, "recording-widget-leaf"));
    }
}

mod redo_transcript {
    use super::*;
    use crate::testing::{meeting_fixture, segment_fixture, speaker_fixture, FakeMeetingsBackend};
    use crate::ui::pages::meetings::MeetingsPage;
    use guidy_core::meetings::types::{CloudTranscriptionOption, MeetingStatus};

    fn has_dialog(cx: &mut TestAppContext, w: AnyWindowHandle) -> bool {
        cx.update_window(w, |_, window, cx| {
            gpui_kit::component::WindowExt::has_active_dialog(window, cx)
        })
        .unwrap()
    }

    /// Meeting 1, processed, with `texts` as its transcript (10 s each);
    /// Groq has a key, OpenAI doesn't.
    fn fixture(texts: &[&str]) -> (Arc<FakeBackend>, Arc<FakeMeetingsBackend>) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| {
            s.meetings.enabled = true;
            s.meetings.meeting_model = Some("qwen".into());
        });
        backend.data.lock().models = ["qwen", "whisper", "parakeet"]
            .map(|id| {
                let mut m = crate::logic::models::test_model(id);
                m.is_downloaded = id != "parakeet";
                m
            })
            .into();
        let m = FakeMeetingsBackend::new();
        {
            let mut d = m.data.lock();
            d.meetings = vec![meeting_fixture(1, "Weekly sync", MeetingStatus::Processed)];
            d.speakers
                .insert(1, vec![speaker_fixture(5, 1, "Speaker 1", None, false)]);
            d.segments.insert(
                1,
                texts
                    .iter()
                    .enumerate()
                    .map(|(i, text)| {
                        let mut s = segment_fixture(i as i64 + 1, 1, 5, i as i64 * 10_000, text);
                        s.t1_ms = s.t0_ms + 10_000;
                        s
                    })
                    .collect(),
            );
            d.cloud_options = vec![
                CloudTranscriptionOption {
                    provider_id: "groq".into(),
                    label: "Groq".into(),
                    model: "whisper-large-v3-turbo".into(),
                    has_key: true,
                },
                CloudTranscriptionOption {
                    provider_id: "openai".into(),
                    label: "OpenAI".into(),
                    model: "whisper-1".into(),
                    has_key: false,
                },
            ];
        }
        (backend, m)
    }

    fn press_enter(cx: &mut TestAppContext, w: AnyWindowHandle) {
        cx.update_window(w, |_, window, cx| window.press("enter", cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn detail(
        cx: &mut TestAppContext,
        page: &Entity<MeetingsPage>,
    ) -> Entity<crate::ui::pages::meetings::detail::MeetingDetail> {
        page.read_with(cx, |p, _| p.detail().cloned())
            .expect("a meeting is open")
    }

    #[gpui_kit::test]
    fn a_processed_transcript_can_be_redone_in_the_cloud(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture(&["Hello everyone, let's get going with the plan."]);
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        assert!(!exists(cx, w, "transcript-quality"), "a good transcript");
        // One click: the model meetings use now, no questions.
        with_window(cx, w, |window, cx| window.click("meeting-detail-redo", cx));
        assert!(!has_dialog(cx, w));
        assert!(
            meetings.called("retranscribe 1 local qwen"),
            "{:?}",
            meetings.calls()
        );

        // The menu: a cloud provider asks first; one without a key does nothing.
        let d = detail(cx, &page);
        let choices = cx.update(|cx| d.read(cx).redo_choices(cx));
        let pick = |provider: &str| {
            choices
                .iter()
                .find(|c| c.cloud && c.provider == provider)
                .cloned()
                .unwrap()
        };
        cx.update_window(w, |_, window, cx| {
            d.update(cx, |d, cx| d.choose_redo(pick("OpenAI"), window, cx))
        })
        .unwrap();
        assert!(!has_dialog(cx, w));
        cx.update_window(w, |_, window, cx| {
            d.update(cx, |d, cx| d.choose_redo(pick("Groq"), window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert!(has_dialog(cx, w));
        assert!(!meetings.called("retranscribe 1 cloud"));
        press_enter(cx, w);
        assert!(
            meetings.called("retranscribe 1 cloud groq whisper-large-v3-turbo"),
            "{:?}",
            meetings.calls()
        );
    }

    #[gpui_kit::test]
    fn the_summary_and_transcript_can_be_copied(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture(&["Let's ship it on Friday."]);
        meetings.data.lock().ai.summaries.insert(
            1,
            guidy_core::meetings::types::StoredSummary {
                meeting_id: 1,
                title: "Weekly sync".into(),
                short_summary: "Shipping Friday.".into(),
                decisions: vec!["Ship on Friday".into()],
                topics: Vec::new(),
                generated_by: "groq/x".into(),
                generated_at: 0,
                usage: None,
            },
        );
        setup_meetings(cx, backend, meetings);
        let (w, _) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        let clipboard = |cx: &mut TestAppContext| {
            cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()))
                .unwrap_or_default()
        };

        with_window(cx, w, |window, cx| window.click("transcript-copy", cx));
        let text = clipboard(cx);
        assert!(text.starts_with("# Weekly sync\n"), "{text}");
        assert!(
            text.contains("[00:00] Speaker 1: Let's ship it on Friday."),
            "{text}"
        );

        with_window(cx, w, |window, cx| window.click("summary-copy", cx));
        let md = clipboard(cx);
        assert!(md.contains("## Summary\n\nShipping Friday."), "{md}");
        assert!(md.contains("## Decisions\n\n- Ship on Friday"), "{md}");
    }

    #[gpui_kit::test]
    fn a_looping_transcript_offers_a_redo(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture(&[
            "thank you thank you thank you thank you",
            "okay so",
            "the the the the the",
        ]);
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        assert!(exists(cx, w, "transcript-quality"));
        with_window(cx, w, |window, cx| {
            window.click("transcript-quality-redo", cx)
        });
        assert!(
            meetings.called("retranscribe 1 local qwen"),
            "{:?}",
            meetings.calls()
        );
        // The menu offers every downloaded model, the current one marked.
        let d = detail(cx, &page);
        let local: Vec<(String, bool)> = cx.update(|cx| {
            d.read(cx)
                .redo_choices(cx)
                .into_iter()
                .filter(|c| !c.cloud)
                .map(|c| (c.model, c.current))
                .collect()
        });
        assert_eq!(local.len(), 2, "{local:?}");
        assert!(local[0].1, "the current model first: {local:?}");
    }
}

mod line_tagging {
    use super::*;
    use crate::testing::{
        meeting_fixture, person_fixture, segment_fixture, speaker_fixture, FakeMeetingsBackend,
    };
    use crate::ui::pages::meetings::MeetingsPage;
    use guidy_core::meetings::types::MeetingStatus;

    /// Meeting 1: Prashanth's line, two lines nobody could place, then
    /// Prashanth again. People: Prashanth (7), Mageshwari (8).
    fn fixture() -> (Arc<FakeBackend>, Arc<FakeMeetingsBackend>) {
        let backend = FakeBackend::new();
        backend.with_settings(|s| s.meetings.enabled = true);
        let m = FakeMeetingsBackend::new();
        {
            let mut d = m.data.lock();
            d.meetings = vec![meeting_fixture(
                1,
                "TCPL stand-up",
                MeetingStatus::Processed,
            )];
            d.people = vec![
                person_fixture(7, "Prashanth", None),
                person_fixture(8, "Mageshwari", None),
            ];
            d.speakers
                .insert(1, vec![speaker_fixture(5, 1, "Speaker 1", Some(7), false)]);
            let unknown = |id: i64, t0: i64, text: &str| {
                let mut s = segment_fixture(id, 1, 5, t0, text);
                s.speaker_id = None;
                s
            };
            d.segments.insert(
                1,
                vec![
                    segment_fixture(1, 1, 5, 0, "Can I do?"),
                    unknown(2, 3_000, "Hi, how are you doing on TCPR?"),
                    unknown(3, 6_000, "And two were related to tech."),
                    segment_fixture(4, 1, 5, 9_000, "Okay, and how is SLS?"),
                ],
            );
        }
        (backend, m)
    }

    /// The standard model (downloaded) and two larger ones (not).
    fn separation_options() -> guidy_core::meetings::types::DiarizationOptions {
        use guidy_core::meetings::types::{DiarizationOptions, SpeakerModelId, SpeakerModelOption};
        let model = |id, name: &str, size_mb, installed| SpeakerModelOption {
            id,
            name: name.into(),
            size_mb,
            installed,
        };
        DiarizationOptions {
            models: vec![
                model(SpeakerModelId::Resnet34, "WeSpeaker ResNet34", 26, true),
                model(
                    SpeakerModelId::TitanetLarge,
                    "NVIDIA TitaNet Large",
                    97,
                    false,
                ),
                model(SpeakerModelId::Resnet293, "WeSpeaker ResNet293", 110, false),
            ],
            default_model: SpeakerModelId::Resnet34,
            deepgram_has_key: false,
            deepgram_usd_per_min: Some(0.0063),
        }
    }

    /// The speaker model that tells voices apart is the user's choice: it
    /// downloads when chosen and can be deleted again.
    #[gpui_kit::test]
    fn the_speaker_model_can_be_chosen_downloaded_and_deleted(cx: &mut TestAppContext) {
        use guidy_core::meetings::types::SpeakerModelId;
        let (backend, meetings) = fixture();
        meetings.data.lock().diarization = separation_options();
        setup_meetings(cx, backend.clone(), meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| {
            window.click("meetings-tab-settings", cx)
        });
        assert!(exists(cx, w, "meetings-separation-model"));
        assert!(!exists(cx, w, "meetings-separation-delete"));
        let view = page.read_with(cx, |p, _| p.settings_view().clone());
        view.update(cx, |v, cx| {
            v.choose_separation_model(SpeakerModelId::TitanetLarge, cx)
        });
        cx.run_until_parked();
        assert!(meetings.called("download_separation_model titanet_large"));
        assert_eq!(
            backend.settings().meetings.speaker_separation_model,
            "titanet_large"
        );
        // Downloaded: it can be deleted, and meetings go back to the
        // standard model.
        assert!(exists(cx, w, "meetings-separation-delete"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-separation-delete", cx)
        });
        assert!(meetings.called("delete_separation_model titanet_large"));
        assert_eq!(backend.settings().meetings.speaker_separation_model, "");
        // The standard one needs no download.
        view.update(cx, |v, cx| {
            v.choose_separation_model(SpeakerModelId::Resnet34, cx)
        });
        cx.run_until_parked();
        assert!(!meetings.called("download_separation_model resnet34"));
    }

    /// Live transcription can use its own model.
    #[gpui_kit::test]
    fn live_transcription_has_its_own_model_choice(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        backend.with_settings(|s| s.meetings.live_transcription = false);
        setup_meetings(cx, backend.clone(), meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| {
            window.click("meetings-tab-settings", cx)
        });
        assert!(!exists(cx, w, "meetings-live-model"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-live-transcription-toggle", cx)
        });
        assert!(exists(cx, w, "meetings-live-model"));
    }

    #[gpui_kit::test]
    fn the_speakers_can_be_redone_locally_or_with_deepgram(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        meetings.data.lock().diarization = separation_options();
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        // One click: the most accurate model (meetings use the standard one).
        with_window(cx, w, |window, cx| window.click("speakers-redo", cx));
        assert!(
            meetings.called("redo_speakers 1 Model(Resnet293)"),
            "{:?}",
            meetings.calls()
        );

        let d = page
            .read_with(cx, |p, _| p.detail().cloned())
            .expect("a meeting is open");
        let deepgram = |cx: &mut TestAppContext| {
            cx.update(|cx| d.read(cx).speaker_redo_choices())
                .into_iter()
                .find(|c| c.cloud)
                .unwrap()
        };
        // Without a key: listed, does nothing.
        let choice = deepgram(cx);
        assert!(!choice.enabled);
        cx.update_window(w, |_, window, cx| {
            d.update(cx, |d, cx| d.choose_speaker_redo(choice, window, cx))
        })
        .unwrap();
        assert!(!meetings.called("redo_speakers 1 Deepgram"));
        // With one: asks first (it uploads the audio).
        meetings.data.lock().diarization.deepgram_has_key = true;
        d.update(cx, |d, cx| d.load(cx));
        cx.run_until_parked();
        let choice = deepgram(cx);
        assert!(choice.enabled);
        cx.update_window(w, |_, window, cx| {
            d.update(cx, |d, cx| d.choose_speaker_redo(choice, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert!(!meetings.called("redo_speakers 1 Deepgram"));
        cx.update_window(w, |_, window, cx| window.press("enter", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(
            meetings.called("redo_speakers 1 Deepgram"),
            "{:?}",
            meetings.calls()
        );
    }

    #[gpui_kit::test]
    fn an_unknown_voice_is_highlighted_and_its_turn_can_be_named(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        // Blocks: Prashanth, the unknown turn, Prashanth.
        assert!(!exists(cx, w, "block-0-tag"));
        assert!(exists(cx, w, "block-1-tag"), "unknown gets 'Who is this?'");
        with_window(cx, w, |window, cx| window.click("block-1-tag", cx));
        assert!(meetings.called("segment_suggestions 2"));
        assert!(
            exists(cx, w, "line-picker-2"),
            "under the turn's first line"
        );
        // Suggestions: Prashanth, then Mageshwari.
        with_window(cx, w, |window, cx| window.click("speaker-suggestion-1", cx));
        assert!(
            meetings.called("assign_segment 2 8"),
            "{:?}",
            meetings.calls()
        );
        assert!(
            meetings.called("assign_segment 3 8"),
            "{:?}",
            meetings.calls()
        );
    }

    /// Several lines are selected (Shift-click a range, Command-click one
    /// more, or drag) and named with one "Set speaker".
    #[gpui_kit::test]
    fn several_lines_are_selected_and_named_at_once(cx: &mut TestAppContext) {
        use crate::logic::meetings::LineClick;
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        let d = page
            .read_with(cx, |p, _| p.detail().cloned())
            .expect("a meeting is open");
        let seg = |cx: &mut TestAppContext, id: i64| {
            cx.update(|cx| {
                d.read(cx)
                    .segments()
                    .iter()
                    .find(|s| s.id == id)
                    .cloned()
                    .unwrap()
            })
        };
        // A plain click plays, and is where a Shift-click range starts.
        with_window(cx, w, |window, cx| window.click("segment-1", cx));
        assert!(meetings.called("segment_audio"));
        assert!(!exists(cx, w, "selection-bar"));
        let three = seg(cx, 3);
        d.update(cx, |d, cx| d.click_line(&three, LineClick::Range, cx));
        cx.run_until_parked();
        assert_eq!(d.read_with(cx, |d, _| d.selection().to_vec()), [1, 2, 3]);
        assert!(exists(cx, w, "selection-bar"));
        // Command-click takes one off again.
        let two = seg(cx, 2);
        d.update(cx, |d, cx| d.click_line(&two, LineClick::Toggle, cx));
        cx.run_until_parked();
        assert_eq!(d.read_with(cx, |d, _| d.selection().to_vec()), [1, 3]);
        d.update(cx, |d, cx| d.click_line(&two, LineClick::Toggle, cx));
        cx.run_until_parked();

        // One picker for all of them, under the last line.
        with_window(cx, w, |window, cx| window.click("selection-tag", cx));
        assert!(exists(cx, w, "line-picker-3"));
        assert!(!exists(cx, w, "selection-bar"));
        with_window(cx, w, |window, cx| window.click("speaker-suggestion-1", cx));
        for id in [1, 2, 3] {
            assert!(
                meetings.called(&format!("assign_segment {id} 8")),
                "{:?}",
                meetings.calls()
            );
        }
        assert!(d.read_with(cx, |d, _| d.selection().is_empty()));

        // Esc drops a selection.
        let one = seg(cx, 1);
        d.update(cx, |d, cx| d.click_line(&one, LineClick::Plain, cx));
        d.update(cx, |d, cx| d.click_line(&three, LineClick::Range, cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "selection-bar"));
        with_window(cx, w, |window, cx| window.click("selection-clear", cx));
        assert!(!exists(cx, w, "selection-bar"));
    }

    /// Dragging across lines selects them (and plays none); Esc drops the
    /// selection.
    #[gpui_kit::test]
    fn dragging_across_lines_selects_them(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, page) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        let d = page
            .read_with(cx, |p, _| p.detail().cloned())
            .expect("a meeting is open");
        with_window(cx, w, |window, cx| {
            window.drag_to("segment-1", "segment-3", cx)
        });
        cx.run_until_parked();
        assert_eq!(d.read_with(cx, |d, _| d.selection().to_vec()), [1, 2, 3]);
        assert!(!meetings.called("segment_audio"), "a drag doesn't play");
        assert!(exists(cx, w, "selection-bar"));
        cx.update_window(w, |_, window, cx| window.press("escape", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(d.read_with(cx, |d, _| d.selection().is_empty()));
        // The next plain click isn't taken for the drag's end: it is where
        // a Shift-click range starts.
        with_window(cx, w, |window, cx| window.click("segment-2", cx));
        let three = cx.update(|cx| {
            d.read(cx)
                .segments()
                .iter()
                .find(|s| s.id == 3)
                .cloned()
                .unwrap()
        });
        d.update(cx, |d, cx| {
            d.click_line(&three, crate::logic::meetings::LineClick::Range, cx)
        });
        assert_eq!(d.read_with(cx, |d, _| d.selection().to_vec()), [2, 3]);
    }

    #[gpui_kit::test]
    fn one_wrongly_named_line_can_be_renamed_without_playing_it(cx: &mut TestAppContext) {
        let (backend, meetings) = fixture();
        setup_meetings(cx, backend, meetings.clone());
        let (w, _) = open(cx, MeetingsPage::new);
        with_window(cx, w, |window, cx| window.click("meeting-open-1", cx));
        with_window(cx, w, |window, cx| window.click("segment-4-tag", cx));
        assert!(meetings.called("segment_suggestions 4"));
        assert!(
            exists(cx, w, "line-picker-4"),
            "right under the clicked line"
        );
        assert!(!meetings.called("segment_audio"), "tagging doesn't play");
        with_window(cx, w, |window, cx| window.click("speaker-suggestion-1", cx));
        assert!(
            meetings.called("assign_segment 4 8"),
            "{:?}",
            meetings.calls()
        );
        // Only that line.
        assert!(!meetings.called("assign_segment 1"));
    }
}
