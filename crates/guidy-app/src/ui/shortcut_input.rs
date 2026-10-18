//! Shortcut recorder row (picking the system or
//! native-keys implementation from the `keyboard_implementation` setting).

use crate::logic::flows::SECURE_INPUT_HELP_URL;
use crate::logic::keyboard::{
    format_key_combination, gpui_key_name, modifier_names, NativeKeysInput, NativeKeysRecorder,
    OsType, SystemRecorder,
};
use crate::state::{AppEvent, AppState};
use crate::ui::design;
use crate::ui::kit::{reset_button, t, t_args, SettingRow};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{h_flex, ActiveTheme, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::settings::KeyboardImplementation;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Idle,
    System,
    NativeKeys,
}

pub struct ShortcutInput {
    binding_id: SharedString,
    state: Entity<AppState>,
    mode: Mode,
    original: String,
    system: SystemRecorder,
    native_keys: NativeKeysRecorder,
    held_modifiers: Vec<&'static str>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl ShortcutInput {
    pub fn new(
        binding_id: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = AppState::global(cx);
        let mut subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(&state, window, |this, _, event: &AppEvent, window, cx| {
                if let AppEvent::NativeKeys(k) = event {
                    if this.mode == Mode::NativeKeys {
                        let input = NativeKeysInput {
                            modifiers: &k.modifiers,
                            key: k.key.as_deref(),
                            is_key_down: k.is_key_down,
                            hotkey_string: &k.hotkey_string,
                        };
                        if let Some(commit) = this.native_keys.on_event(&input) {
                            this.commit(commit, window, cx);
                        }
                        cx.notify();
                    }
                }
            }),
        ];
        // Closing the window mid-recording must not leave every shortcut
        // suspended (or the native-keys recorder running).
        subs.push(cx.on_release(|this: &mut Self, cx: &mut App| {
            let backend = this.state.read(cx).backend.clone();
            release(this.mode, backend, cx);
        }));
        Self {
            binding_id: binding_id.into(),
            state,
            mode: Mode::Idle,
            original: String::new(),
            system: SystemRecorder::default(),
            native_keys: NativeKeysRecorder::default(),
            held_modifiers: Vec::new(),
            focus: cx.focus_handle(),
            _subs: subs,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.mode != Mode::Idle
    }

    fn implementation(&self, cx: &App) -> KeyboardImplementation {
        self.state
            .read(cx)
            .app_settings()
            .map(|s| s.keyboard_implementation)
            .unwrap_or_default()
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_recording() {
            return;
        }
        let id = self.binding_id.to_string();
        self.original = self
            .state
            .read(cx)
            .app_settings()
            .and_then(|s| s.bindings.get(&id).map(|b| b.current_binding.clone()))
            .unwrap_or_default();
        self.system = SystemRecorder::default();
        self.native_keys = NativeKeysRecorder::default();
        self.held_modifiers.clear();
        match self.implementation(cx) {
            KeyboardImplementation::NativeKeys => {
                let backend = self.state.read(cx).backend.clone();
                let result = backend.start_native_keys_recording(&id);
                match result {
                    Ok(()) => self.mode = Mode::NativeKeys,
                    Err(e) if e.contains("secure-input-active") => {
                        window.push_notification(
                            Notification::new()
                                .title(t("secureInput.recorderBlocked"))
                                .with_type(NotificationType::Error)
                                .action(|_, _, _| {
                                    gpui_kit::component::button::Button::new("secure-learn-more")
                                        .label(t("secureInput.learnMore"))
                                        .on_click(|_, _, cx| cx.open_url(SECURE_INPUT_HELP_URL))
                                }),
                            cx,
                        );
                    }
                    Err(e) => window.push_notification(
                        (
                            NotificationType::Error,
                            t_args("settings.general.shortcut.errors.set", &[("error", &e)]),
                        ),
                        cx,
                    ),
                }
            }
            KeyboardImplementation::Tauri => {
                // Suspend every shortcut so none fires (or swallows the
                // keystrokes) while recording.
                let backend = self.state.read(cx).backend.clone();
                cx.background_spawn(async move {
                    if let Err(e) = backend.suspend_all_bindings() {
                        log::error!("Failed to suspend bindings: {e}");
                    }
                })
                .detach();
                self.mode = Mode::System;
                window.focus(&self.focus, cx);
            }
        }
        cx.notify();
    }

    /// End recording locally; returns the mode whose backend side still
    /// needs releasing.
    fn reset(&mut self, cx: &mut Context<Self>) -> Mode {
        let mode = self.mode;
        self.mode = Mode::Idle;
        self.system = SystemRecorder::default();
        self.native_keys = NativeKeysRecorder::default();
        self.held_modifiers.clear();
        self.original.clear();
        cx.notify();
        mode
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let backend = self.state.read(cx).backend.clone();
        let mode = self.reset(cx);
        release(mode, backend, cx);
    }

    /// Write `binding`, then release the recorder, so resuming bindings
    /// can't race the change and leave the old shortcut registered. On
    /// failure (and when `rollback` is set) the original is restored first.
    fn write_then_release(
        &mut self,
        binding: String,
        rollback: Option<String>,
        error_key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.binding_id.to_string();
        let window_handle = window.window_handle();
        let backend = self.state.read(cx).backend.clone();
        let mode = self.reset(cx);
        let id_cb = id.clone();
        self.state.update(cx, |s, cx| {
            s.update_binding(&id, &binding, cx, move |s, result, cx| {
                let id = id_cb;
                let Err(e) = result else {
                    release(mode, backend, cx);
                    return;
                };
                let message = if error_key == "settings.general.shortcut.errors.set" {
                    t_args(error_key, &[("error", &e)])
                } else {
                    t(error_key)
                };
                notify_error(window_handle, message, cx);
                let Some(original) = rollback.filter(|o| !o.is_empty()) else {
                    release(mode, backend, cx);
                    return;
                };
                let id_cb = id.clone();
                s.update_binding(&id, &original, cx, move |_, r, cx| {
                    if r.is_err() {
                        log::error!("Failed to reset binding {id_cb}");
                        notify_error(
                            window_handle,
                            t("settings.general.shortcut.errors.reset"),
                            cx,
                        );
                    }
                    release(mode, backend, cx);
                });
            });
        });
    }

    fn commit(&mut self, binding: String, window: &mut Window, cx: &mut Context<Self>) {
        let original = self.original.clone();
        self.write_then_release(
            binding,
            Some(original),
            "settings.general.shortcut.errors.set",
            window,
            cx,
        );
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_recording() {
            return;
        }
        let original = self.original.clone();
        if original.is_empty() {
            self.stop(cx);
            return;
        }
        self.write_then_release(
            original,
            None,
            "settings.general.shortcut.errors.restore",
            window,
            cx,
        );
    }

    fn on_key_down(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::System {
            return;
        }
        let key = gpui_key_name(&e.keystroke.key, OsType::current());
        self.system.key_down(&key, e.is_held);
        cx.stop_propagation();
        cx.notify();
    }

    fn on_key_up(&mut self, e: &KeyUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::System {
            return;
        }
        let key = gpui_key_name(&e.keystroke.key, OsType::current());
        if let Some(commit) = self.system.key_up(&key) {
            self.commit(commit, window, cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn on_modifiers(
        &mut self,
        e: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode != Mode::System {
            return;
        }
        let m = e.modifiers;
        let now = modifier_names(
            m.control,
            m.alt,
            m.shift,
            m.platform,
            m.function,
            OsType::current(),
        );
        for added in now.iter().filter(|k| !self.held_modifiers.contains(k)) {
            self.system.key_down(added, false);
        }
        let released: Vec<&'static str> = self
            .held_modifiers
            .iter()
            .filter(|k| !now.contains(k))
            .copied()
            .collect();
        self.held_modifiers = now;
        for r in released {
            if let Some(commit) = self.system.key_up(r) {
                self.commit(commit, window, cx);
                break;
            }
        }
        cx.notify();
    }
}

impl Render for ShortcutInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.binding_id.to_string();
        let st = self.state.read(cx);
        let theme = cx.theme();
        let row_id = SharedString::from(format!("shortcut-{id}"));
        let Some(settings) = st.app_settings() else {
            return SettingRow::new(row_id, t("settings.general.shortcut.title"))
                .description(t("settings.general.shortcut.description"))
                .control(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(t("settings.general.shortcut.loading")),
                )
                .into_any_element();
        };
        if settings.bindings.is_empty() {
            return SettingRow::new(row_id, t("settings.general.shortcut.title"))
                .description(t("settings.general.shortcut.description"))
                .control(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(t("settings.general.shortcut.none")),
                )
                .into_any_element();
        }
        let Some(binding) = settings.bindings.get(&id).cloned() else {
            return SettingRow::new(row_id, t("settings.general.shortcut.title"))
                .description(t("settings.general.shortcut.notFound"))
                .control(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(t("settings.general.shortcut.none")),
                )
                .into_any_element();
        };
        let name_key = format!("settings.general.shortcut.bindings.{id}.name");
        let desc_key = format!("settings.general.shortcut.bindings.{id}.description");
        let title: SharedString = if guidy_i18n::exists(&name_key) {
            t(&name_key)
        } else {
            binding.name.clone().into()
        };
        let desc: SharedString = if guidy_i18n::exists(&desc_key) {
            t(&desc_key)
        } else {
            binding.description.clone().into()
        };
        let updating = st.settings.is_updating(&format!("binding_{id}"));

        let recording = self.is_recording();
        let display: SharedString = if recording {
            let current = match self.mode {
                Mode::System => self.system.display(),
                Mode::NativeKeys => (!self.native_keys.current.is_empty())
                    .then(|| format_key_combination(&self.native_keys.current)),
                Mode::Idle => None,
            };
            current
                .map(Into::into)
                .unwrap_or_else(|| t("settings.general.shortcut.pressKeys"))
        } else {
            format_key_combination(&binding.current_binding).into()
        };
        let tone = design::tone(cx);
        let input_border = theme.input;
        // One key cap per key; while recording, a single highlighted cap
        // shows what's held so far.
        let cap = move |text: SharedString| {
            div()
                .px(px(8.))
                .py(px(5.))
                .rounded(px(6.))
                .border_1()
                .border_color(input_border)
                .bg(tone.raised)
                .font_family(crate::theme::MONO_FONT)
                .text_size(px(11.5))
                .line_height(px(14.))
                .child(text)
        };
        let caps: Vec<SharedString> = if recording {
            vec![display.clone()]
        } else {
            display
                .split(" + ")
                .map(|k| SharedString::from(with_key_glyph(k)))
                .collect()
        };

        let chip = h_flex()
            .id(SharedString::from(format!("shortcut-chip-{id}")))
            .aria_label(title.clone())
            .track_focus(&self.focus)
            .gap(px(4.))
            .p(px(2.))
            .rounded(px(8.))
            .border_1()
            .cursor_pointer()
            .children(caps.into_iter().map(move |k| {
                cap(k).when(recording, |d| {
                    d.border_color(tone.accent_text)
                        .bg(tone.accent_soft)
                        .text_color(tone.accent_text)
                })
            }))
            .when(recording, |d| d.border_color(tone.accent_soft))
            .when(!recording, |d| {
                d.border_color(gpui_kit::transparent_black())
                    .hover(move |s| s.border_color(tone.accent_soft))
            })
            .on_click(cx.listener(|this, _, window, cx| this.start(window, cx)))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.cancel(window, cx)))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            .test_support();

        let reset_id = id.clone();
        SettingRow::new(row_id, title)
            .description(desc)
            .control(h_flex().gap_1().child(chip).child(reset_button(
                SharedString::from(format!("shortcut-reset-{id}")),
                updating,
                move |_, cx| {
                    let id = reset_id.clone();
                    AppState::global(cx).update(cx, |s, cx| s.reset_binding(&id, cx));
                },
            )))
            .into_any_element()
    }
}

/// "Left Option" → "⌥ Left Option": the Mac modifier symbol in front of
/// its name, as printed on the key.
fn with_key_glyph(key: &str) -> String {
    let name = key.rsplit(' ').next().unwrap_or(key).to_ascii_lowercase();
    let glyph = match name.as_str() {
        "option" | "alt" => "⌥",
        "command" | "cmd" | "super" | "meta" => "⌘",
        "shift" => "⇧",
        "control" | "ctrl" => "⌃",
        _ => return key.to_string(),
    };
    format!("{glyph} {key}")
}

/// Give the keyboard back: resume suspended bindings (system recorder) or
/// stop the native-keys recorder.
fn release(mode: Mode, backend: crate::backend::SharedBackend, cx: &mut App) {
    match mode {
        Mode::NativeKeys => cx
            .background_spawn(async move {
                let _ = backend.stop_native_keys_recording();
            })
            .detach(),
        Mode::System => cx
            .background_spawn(async move {
                let _ = backend.resume_all_bindings();
            })
            .detach(),
        Mode::Idle => {}
    }
}

fn notify_error(window: AnyWindowHandle, message: SharedString, cx: &mut App) {
    let _ = cx.update_window(window, |_, window, cx| {
        window.push_notification((NotificationType::Error, message), cx);
    });
}

#[cfg(test)]
mod tests {
    use super::with_key_glyph;

    #[test]
    fn modifiers_get_their_mac_symbol() {
        assert_eq!(with_key_glyph("Left Option"), "⌥ Left Option");
        assert_eq!(with_key_glyph("Command"), "⌘ Command");
        assert_eq!(with_key_glyph("Shift"), "⇧ Shift");
        assert_eq!(with_key_glyph("Space"), "Space");
    }
}
