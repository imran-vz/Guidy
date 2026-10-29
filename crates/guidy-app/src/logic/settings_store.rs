//! Settings state: optimistic
//! updates with rollback, per-key "updating" flags, defaults for reset.
//! Pure; the GPUI layer runs the backend calls.

use guidy_core::settings::AppSettings;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct SettingsStore {
    pub settings: Option<AppSettings>,
    pub defaults: Option<AppSettings>,
    /// Keys with an operation running (set/cleared by the caller).
    pub updating: HashSet<String>,
    /// In-flight [`SettingsStore::begin_update`] calls per key. Patched
    /// settings count under the key (`meetings`) and under each patched
    /// field (`meetings.<field>`), so concurrent patches each hold their own
    /// count and the key stays "updating" until the last one finishes.
    in_flight: HashMap<String, usize>,
    /// The last failed write per key, shown inline next to its control.
    /// Cleared when the key is written again.
    errors: HashMap<String, String>,
}

/// Settings whose value is an object patched field by field: the value
/// passed to `update_setting` holds only the changed fields.
pub const PATCHED_SETTINGS: &[&str] = &["meetings"];

/// Write `value` under `key` of a settings JSON object. For
/// [`PATCHED_SETTINGS`] an object value is merged (top-level fields
/// replace, others stay); anything else replaces the value.
pub fn apply_setting_value(settings: &mut Value, key: &str, value: Value) {
    if PATCHED_SETTINGS.contains(&key) {
        if let (Some(Value::Object(current)), Value::Object(patch)) =
            (settings.get_mut(key), &value)
        {
            for (k, v) in patch {
                current.insert(k.clone(), v.clone());
            }
            return;
        }
    }
    settings[key] = value;
}

/// Rollback token for an optimistic update.
#[derive(Clone, Debug)]
pub struct Pending {
    pub key: String,
    /// The value before the update. For [`PATCHED_SETTINGS`], an object of
    /// only the patched fields' previous values.
    pub original: Option<Value>,
    /// The optimistic value as applied (for patched settings, the patched
    /// fields as stored). Rollback only restores what still holds it, so a
    /// later write is never undone.
    applied: Option<Value>,
    /// `in_flight` keys this update holds.
    held: Vec<String>,
}

/// The fields of a [`PATCHED_SETTINGS`] object update, or `None` when `key`
/// replaces its whole value.
fn patched_fields(key: &str, value: &Value) -> Option<Vec<String>> {
    if !PATCHED_SETTINGS.contains(&key) {
        return None;
    }
    value.as_object().map(|o| o.keys().cloned().collect())
}

/// The listed fields of an object value.
fn pick_fields(value: Option<Value>, fields: &[String]) -> Option<Value> {
    let Some(Value::Object(o)) = value else {
        return None;
    };
    let picked: Map<String, Value> = fields
        .iter()
        .filter_map(|f| o.get(f).map(|v| (f.clone(), v.clone())))
        .collect();
    Some(Value::Object(picked))
}

impl SettingsStore {
    pub fn get(&self, key: &str) -> Option<Value> {
        let s = self.settings.as_ref()?;
        serde_json::to_value(s).ok()?.get(key).cloned()
    }

    pub fn default_of(&self, key: &str) -> Option<Value> {
        let s = self.defaults.as_ref()?;
        serde_json::to_value(s).ok()?.get(key).cloned()
    }

    /// Why the last write of `key` failed (its value was rolled back), until
    /// the next write of `key`.
    pub fn error_of(&self, key: &str) -> Option<&str> {
        self.errors.get(key).map(String::as_str)
    }

    /// Record a failed operation on `key` that has no optimistic value
    /// (provider fields, model lists): shown like a failed write.
    pub fn set_error(&mut self, key: &str, error: String) {
        self.errors.insert(key.to_string(), error);
    }

    pub fn clear_error(&mut self, key: &str) {
        self.errors.remove(key);
    }

    pub fn is_updating(&self, key: &str) -> bool {
        self.updating.contains(key) || self.in_flight.get(key).is_some_and(|n| *n > 0)
    }

    fn set_value(&mut self, key: &str, value: Value) -> bool {
        let Some(s) = self.settings.as_ref() else {
            return false;
        };
        let Ok(mut json) = serde_json::to_value(s) else {
            return false;
        };
        apply_setting_value(&mut json, key, value);
        match serde_json::from_value::<AppSettings>(json) {
            Ok(updated) => {
                self.settings = Some(updated);
                true
            }
            Err(e) => {
                log::warn!("Rejected optimistic value for {key}: {e}");
                false
            }
        }
    }

    /// Apply `value` immediately and mark `key` updating. For
    /// [`PATCHED_SETTINGS`], `value` holds only the changed fields, and each
    /// field (`<key>.<field>`) is marked too.
    pub fn begin_update(&mut self, key: &str, value: Value) -> Pending {
        let fields = patched_fields(key, &value);
        let held: Vec<String> = std::iter::once(key.to_string())
            .chain(fields.iter().flatten().map(|f| format!("{key}.{f}")))
            .collect();
        for k in &held {
            *self.in_flight.entry(k.clone()).or_default() += 1;
        }
        self.errors.remove(key);
        let snapshot = |this: &Self| match &fields {
            Some(fields) => pick_fields(this.get(key), fields),
            None => this.get(key),
        };
        let original = snapshot(self);
        let applied = self.set_value(key, value).then(|| snapshot(self)).flatten();
        Pending {
            key: key.to_string(),
            original,
            applied,
            held,
        }
    }

    /// The backend call finished. On failure, roll back what this update
    /// changed (only the patched fields of a patched setting), and only where
    /// the value is still the optimistic one: a newer update wins.
    pub fn finish_update(&mut self, pending: Pending, result: &Result<(), String>) {
        if let Err(e) = result {
            log::error!("Failed to update setting {}: {}", pending.key, e);
            self.roll_back(&pending);
            self.errors.insert(pending.key.clone(), e.clone());
        }
        for k in &pending.held {
            if let Some(n) = self.in_flight.get_mut(k) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    self.in_flight.remove(k);
                }
            }
        }
    }

    fn roll_back(&mut self, pending: &Pending) {
        let (Some(original), Some(applied)) = (&pending.original, &pending.applied) else {
            return;
        };
        let key = pending.key.as_str();
        if PATCHED_SETTINGS.contains(&key) {
            let (Value::Object(original), Value::Object(applied)) = (original, applied) else {
                return;
            };
            let current = self.get(key);
            let restore: Map<String, Value> = original
                .iter()
                .filter(|(f, _)| {
                    current.as_ref().and_then(|c| c.get(f.as_str())) == applied.get(f.as_str())
                })
                .map(|(f, v)| (f.clone(), v.clone()))
                .collect();
            if !restore.is_empty() {
                self.set_value(key, Value::Object(restore));
            }
        } else if self.get(key).as_ref() == Some(applied) {
            self.set_value(key, original.clone());
        }
    }

    /// Optimistic binding change (key `binding_<id>`).
    pub fn begin_binding(&mut self, id: &str, binding: &str) -> Option<String> {
        self.updating.insert(format!("binding_{id}"));
        let s = self.settings.as_mut()?;
        let b = s.bindings.get_mut(id)?;
        let original = std::mem::replace(&mut b.current_binding, binding.to_string());
        Some(original)
    }

    pub fn finish_binding(&mut self, id: &str, original: Option<String>, ok: bool) {
        if !ok {
            if let (Some(orig), Some(s)) = (original, self.settings.as_mut()) {
                if let Some(b) = s.bindings.get_mut(id) {
                    b.current_binding = orig;
                }
            }
        }
        self.updating.remove(&format!("binding_{id}"));
    }
}

/// A toast to show in the main window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastSpec {
    pub kind: ToastKind,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Error,
    Warning,
    Success,
    Info,
}

impl ToastSpec {
    pub fn error(title: impl Into<String>, description: Option<String>) -> Self {
        Self {
            kind: ToastKind::Error,
            title: title.into(),
            description,
        }
    }
}

pub fn platform_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// Global error toasts for backend error events.
pub fn toast_for_event(name: &str, payload: &Value) -> Option<ToastSpec> {
    use guidy_i18n::{exists, t, t_args};
    match name {
        "recording-error" => {
            let error_type = payload["error_type"].as_str().unwrap_or_default();
            let detail = payload["detail"].as_str();
            Some(match error_type {
                "microphone_permission_denied" => {
                    let key = format!("errors.micPermissionDenied.{}", platform_key());
                    let description = if exists(&key) {
                        t(&key)
                    } else {
                        t("errors.micPermissionDenied.generic")
                    };
                    ToastSpec::error(t("errors.micPermissionDeniedTitle"), Some(description))
                }
                "no_input_device" => ToastSpec::error(
                    t("errors.noInputDeviceTitle"),
                    Some(t("errors.noInputDevice")),
                ),
                _ => ToastSpec::error(
                    t_args(
                        "errors.recordingFailed",
                        &[("error", detail.unwrap_or("Unknown error"))],
                    ),
                    None,
                ),
            })
        }
        "paste-error" => Some(ToastSpec::error(
            t("errors.pasteFailedTitle"),
            Some(t("errors.pasteFailed")),
        )),
        "transcription-error" => Some(ToastSpec::error(
            t("errors.transcriptionFailedTitle"),
            payload.as_str().map(str::to_string),
        )),
        "model-state-changed" if payload["event_type"] == "loading_failed" => {
            let unknown = t("errors.modelLoadFailedUnknown");
            let model = payload["model_name"]
                .as_str()
                .unwrap_or(&unknown)
                .to_string();
            Some(ToastSpec::error(
                t_args("errors.modelLoadFailed", &[("model", &model)]),
                payload["error"].as_str().map(str::to_string),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> SettingsStore {
        let s = guidy_core::settings::get_default_settings();
        SettingsStore {
            settings: Some(s.clone()),
            defaults: Some(s),
            ..Default::default()
        }
    }

    #[test]
    fn optimistic_update_applies_and_commits() {
        let mut s = store();
        let p = s.begin_update("audio_feedback", json!(true));
        assert!(s.is_updating("audio_feedback"));
        assert_eq!(s.get("audio_feedback"), Some(json!(true)));
        s.finish_update(p, &Ok(()));
        assert!(!s.is_updating("audio_feedback"));
        assert_eq!(s.get("audio_feedback"), Some(json!(true)));
    }

    #[test]
    fn failed_update_rolls_back() {
        let mut s = store();
        let before = s.get("paste_delay_ms");
        let p = s.begin_update("paste_delay_ms", json!(999));
        s.finish_update(p, &Err("nope".into()));
        assert_eq!(s.get("paste_delay_ms"), before);
        assert!(!s.is_updating("paste_delay_ms"));
    }

    #[test]
    fn failed_update_keeps_its_error_until_the_next_write() {
        let mut s = store();
        let p = s.begin_update("audio_feedback", json!(true));
        s.finish_update(p, &Err("disk full".into()));
        assert_eq!(s.error_of("audio_feedback"), Some("disk full"));
        let p = s.begin_update("audio_feedback", json!(true));
        assert_eq!(s.error_of("audio_feedback"), None);
        s.finish_update(p, &Ok(()));
        assert_eq!(s.error_of("audio_feedback"), None);
    }

    #[test]
    fn reset_uses_defaults() {
        let mut s = store();
        s.begin_update("history_limit", json!(77));
        assert_eq!(s.default_of("history_limit"), Some(json!(5)));
    }

    #[test]
    fn binding_rollback_restores_the_original() {
        let mut s = store();
        let orig = s.begin_binding("transcribe", "ctrl+x");
        assert!(s.is_updating("binding_transcribe"));
        assert_eq!(
            s.settings.as_ref().unwrap().bindings["transcribe"].current_binding,
            "ctrl+x"
        );
        s.finish_binding("transcribe", orig.clone(), false);
        assert_eq!(
            s.settings.as_ref().unwrap().bindings["transcribe"].current_binding,
            orig.unwrap()
        );
        assert!(!s.is_updating("binding_transcribe"));
    }

    #[test]
    fn error_toasts_match_the_spec() {
        let t = toast_for_event(
            "recording-error",
            &json!({"error_type": "no_input_device", "detail": null}),
        )
        .unwrap();
        assert_eq!(t.title, guidy_i18n::t("errors.noInputDeviceTitle"));
        let t = toast_for_event(
            "recording-error",
            &json!({"error_type": "boom", "detail": "x"}),
        )
        .unwrap();
        assert!(t.title.contains('x'));
        let t = toast_for_event(
            "recording-error",
            &json!({"error_type": "microphone_permission_denied"}),
        )
        .unwrap();
        assert_eq!(t.title, guidy_i18n::t("errors.micPermissionDeniedTitle"));
        assert!(t.description.is_some());
        assert!(toast_for_event("paste-error", &Value::Null).is_some());
        let t = toast_for_event("transcription-error", &json!("engine crashed")).unwrap();
        assert_eq!(t.description.as_deref(), Some("engine crashed"));
        let t = toast_for_event(
            "model-state-changed",
            &json!({"event_type": "loading_failed", "model_name": "Tiny", "error": "oom"}),
        )
        .unwrap();
        assert!(t.title.contains("Tiny"));
        assert!(toast_for_event(
            "model-state-changed",
            &json!({"event_type": "loading_completed"})
        )
        .is_none());
    }

    #[test]
    fn meetings_values_merge_field_by_field() {
        let mut st = store();
        st.begin_update("meetings", json!({ "user_display_name": "Imran" }));
        st.begin_update("meetings", json!({ "enabled": true }));
        let m = &st.settings.as_ref().unwrap().meetings;
        assert!(m.enabled);
        assert_eq!(m.user_display_name, "Imran");
        // Other keys still replace their value.
        let mut v = json!({ "a": { "x": 1 } });
        apply_setting_value(&mut v, "a", json!({ "y": 2 }));
        assert_eq!(v, json!({ "a": { "y": 2 } }));
        let mut v = json!({ "meetings": { "x": 1 } });
        apply_setting_value(&mut v, "meetings", json!({ "y": 2 }));
        assert_eq!(v, json!({ "meetings": { "x": 1, "y": 2 } }));
    }

    fn meetings(st: &SettingsStore) -> &guidy_core::meetings::settings::MeetingSettings {
        &st.settings.as_ref().unwrap().meetings
    }

    #[test]
    fn failed_meetings_patch_rolls_back_only_its_fields() {
        let mut st = store();
        let name_before = meetings(&st).user_display_name.clone();
        let a = st.begin_update("meetings", json!({ "user_display_name": "Imran" }));
        let b = st.begin_update("meetings", json!({ "enabled": true }));
        // `b` succeeds, `a` fails: `enabled` must stay true.
        st.finish_update(b, &Ok(()));
        st.finish_update(a, &Err("disk full".into()));
        assert!(meetings(&st).enabled);
        assert_eq!(meetings(&st).user_display_name, name_before);
        assert!(!st.is_updating("meetings"));
    }

    #[test]
    fn rollback_skips_fields_a_newer_patch_changed() {
        let mut st = store();
        let a = st.begin_update("meetings", json!({ "user_display_name": "A" }));
        let b = st.begin_update("meetings", json!({ "user_display_name": "B" }));
        st.finish_update(a, &Err("nope".into()));
        assert_eq!(meetings(&st).user_display_name, "B");
        st.finish_update(b, &Ok(()));
        assert_eq!(meetings(&st).user_display_name, "B");
        // Whole-value settings follow the same rule.
        let a = st.begin_update("paste_delay_ms", json!(10));
        let b = st.begin_update("paste_delay_ms", json!(20));
        st.finish_update(a, &Err("nope".into()));
        assert_eq!(st.get("paste_delay_ms"), Some(json!(20)));
        st.finish_update(b, &Ok(()));
    }

    #[test]
    fn concurrent_patches_keep_the_key_updating_until_all_finish() {
        let mut st = store();
        let a = st.begin_update("meetings", json!({ "user_display_name": "Imran" }));
        let b = st.begin_update("meetings", json!({ "enabled": true }));
        assert!(st.is_updating("meetings"));
        assert!(st.is_updating("meetings.user_display_name"));
        assert!(st.is_updating("meetings.enabled"));
        st.finish_update(a, &Ok(()));
        assert!(st.is_updating("meetings"), "b is still in flight");
        assert!(!st.is_updating("meetings.user_display_name"));
        assert!(st.is_updating("meetings.enabled"));
        st.finish_update(b, &Ok(()));
        assert!(!st.is_updating("meetings"));
        assert!(!st.is_updating("meetings.enabled"));
    }
}
