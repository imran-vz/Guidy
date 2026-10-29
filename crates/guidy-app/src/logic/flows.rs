//! Small UI rules: startup step
//! machine (onboarding screens), option matrices, banners, release notes, updater fallbacks.

use guidy_core::commands::audio::{PermissionAccess, WindowsMicrophonePermissionStatus};
use guidy_core::llm::ProviderRef;
use guidy_core::secure_input::SecureInputStatus;
use guidy_core::settings::{AppSettings, AutoSubmitKey, PasteMethod};
use std::collections::BTreeSet;

use super::keyboard::OsType;

// ---------------------------------------------------------------------------
// Startup / onboarding step machine (App.tsx)
// ---------------------------------------------------------------------------

/// A screen of the startup flow. New users: Welcome → Permissions → Setup
/// → Done. Returning users: Permissions (when one was revoked) and/or
/// Migration (a retired model is selected) → Done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingStep {
    /// What Guidy is; which features to set up.
    Welcome,
    /// Microphone and (for dictation, macOS) Accessibility.
    Permissions,
    /// "Choose your setup": the models (or cloud) each chosen feature needs.
    Setup,
    /// A retired model is selected: download its replacement (or later).
    Migration,
    Done,
}

/// What the startup check knows.
#[derive(Clone, Debug, Default)]
pub struct StartupFacts {
    /// `None` when settings couldn't be read (treated as a new user).
    pub onboarding_completed: Option<bool>,
    /// The selected model is retired and needs migrating.
    pub migration_model_id: Option<String>,
    /// macOS: accessibility and microphone both granted (`None` = check failed).
    pub mac_permissions_ok: Option<bool>,
    /// Windows microphone status (`None` = not Windows or check failed).
    pub windows_mic: Option<WindowsMicrophonePermissionStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupOutcome {
    pub step: OnboardingStep,
    pub returning_user: bool,
    /// Reveal the (possibly hidden) main window for this step.
    pub reveal_window: bool,
}

pub fn initial_step(f: &StartupFacts, os: OsType) -> StartupOutcome {
    if f.onboarding_completed != Some(true) {
        // A new user must see the setup even when launched hidden (login
        // item, `--start-hidden`): nothing works before it.
        return StartupOutcome {
            step: OnboardingStep::Welcome,
            returning_user: false,
            reveal_window: true,
        };
    }
    let needs_permissions = match os {
        OsType::MacOs => f.mac_permissions_ok == Some(false),
        OsType::Windows => f
            .windows_mic
            .as_ref()
            .is_some_and(|s| s.supported && s.overall_access == PermissionAccess::Denied),
        _ => false,
    };
    if needs_permissions {
        return StartupOutcome {
            step: OnboardingStep::Permissions,
            returning_user: true,
            reveal_window: true,
        };
    }
    finish_returning_user(f.migration_model_id.is_some())
}

fn finish_returning_user(needs_migration: bool) -> StartupOutcome {
    StartupOutcome {
        step: if needs_migration {
            OnboardingStep::Migration
        } else {
            OnboardingStep::Done
        },
        returning_user: true,
        reveal_window: needs_migration,
    }
}

/// After the permissions step completes.
pub fn after_permissions(returning_user: bool, migration_model_id: Option<&str>) -> StartupOutcome {
    if returning_user {
        finish_returning_user(migration_model_id.is_some())
    } else {
        StartupOutcome {
            step: OnboardingStep::Setup,
            returning_user: false,
            reveal_window: false,
        }
    }
}

/// Where Back goes from `step` (`None`: no Back, the step is the first).
pub fn step_back(step: OnboardingStep, returning_user: bool) -> Option<OnboardingStep> {
    match step {
        OnboardingStep::Permissions if !returning_user => Some(OnboardingStep::Welcome),
        OnboardingStep::Setup => Some(OnboardingStep::Permissions),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Option matrices
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionSpec {
    pub value: String,
    /// i18n key plus interpolation args.
    pub label_key: &'static str,
    pub label_args: Vec<(&'static str, String)>,
    pub disabled: bool,
}

impl OptionSpec {
    fn new(value: &str, label_key: &'static str) -> Self {
        Self {
            value: value.into(),
            label_key,
            label_args: vec![],
            disabled: false,
        }
    }

    pub fn label(&self) -> String {
        let args: Vec<(&str, &str)> = self
            .label_args
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        guidy_i18n::t_args(self.label_key, &args)
    }
}

/// PasteMethod.tsx options for the OS (with the saved value).
pub fn paste_method_options(os: OsType, selected: PasteMethod) -> Vec<OptionSpec> {
    let modifier = if os == OsType::MacOs { "Cmd" } else { "Ctrl" };
    let mut v = vec![OptionSpec {
        label_args: vec![("modifier", modifier.into())],
        ..OptionSpec::new("ctrl_v", "settings.advanced.pasteMethod.options.clipboard")
    }];
    // Direct input isn't offered on macOS, but keep an existing selection
    // visible (disabled) so the UI represents the saved setting.
    if os != OsType::MacOs || selected == PasteMethod::Direct {
        v.push(OptionSpec {
            disabled: os == OsType::MacOs,
            ..OptionSpec::new("direct", "settings.advanced.pasteMethod.options.direct")
        });
    }
    v.push(OptionSpec::new(
        "none",
        "settings.advanced.pasteMethod.options.none",
    ));
    if matches!(os, OsType::Windows | OsType::Linux) {
        v.push(OptionSpec::new(
            "ctrl_shift_v",
            "settings.advanced.pasteMethod.options.clipboardCtrlShiftV",
        ));
        v.push(OptionSpec::new(
            "shift_insert",
            "settings.advanced.pasteMethod.options.clipboardShiftInsert",
        ));
    }
    if os == OsType::Linux {
        v.push(OptionSpec::new(
            "external_script",
            "settings.advanced.pasteMethod.options.externalScript",
        ));
    }
    v
}

/// The single auto-submit dropdown value (`off` or the key).
pub fn auto_submit_value(enabled: bool, key: AutoSubmitKey) -> &'static str {
    if !enabled {
        return "off";
    }
    match key {
        AutoSubmitKey::Enter => "enter",
        AutoSubmitKey::CtrlEnter => "ctrl_enter",
        AutoSubmitKey::CmdEnter => "cmd_enter",
    }
}

/// Setting writes for a dropdown choice: `off` disables; a key is written
/// first, then `auto_submit` is enabled if it was off.
pub fn auto_submit_writes(
    choice: &str,
    currently_enabled: bool,
) -> Vec<(&'static str, serde_json::Value)> {
    if choice == "off" {
        return vec![("auto_submit", serde_json::json!(false))];
    }
    let mut writes = vec![("auto_submit_key", serde_json::json!(choice))];
    if !currently_enabled {
        writes.push(("auto_submit", serde_json::json!(true)));
    }
    writes
}

pub fn auto_submit_options(os: OsType) -> Vec<OptionSpec> {
    vec![
        OptionSpec::new("off", "settings.advanced.autoSubmit.options.off"),
        OptionSpec::new("enter", "settings.advanced.autoSubmit.options.enter"),
        OptionSpec::new(
            "ctrl_enter",
            "settings.advanced.autoSubmit.options.ctrlEnter",
        ),
        OptionSpec::new(
            "cmd_enter",
            if os == OsType::MacOs {
                "settings.advanced.autoSubmit.options.cmdEnter"
            } else {
                "settings.advanced.autoSubmit.options.superEnter"
            },
        ),
    ]
}

/// Model unload timeout options (15 s only in debug mode).
pub fn model_unload_options(debug_mode: bool) -> Vec<OptionSpec> {
    let mut v: Vec<OptionSpec> = [
        "never",
        "immediately",
        "min2",
        "min5",
        "min10",
        "min15",
        "hour1",
    ]
    .iter()
    .map(|o| OptionSpec::new(o, unload_key(o)))
    .collect();
    if debug_mode {
        v.push(OptionSpec::new("sec15", unload_key("sec15")));
    }
    v
}

fn unload_key(o: &str) -> &'static str {
    match o {
        "never" => "settings.advanced.modelUnload.options.never",
        "immediately" => "settings.advanced.modelUnload.options.immediately",
        "min2" => "settings.advanced.modelUnload.options.min2",
        "min5" => "settings.advanced.modelUnload.options.min5",
        "min10" => "settings.advanced.modelUnload.options.min10",
        "min15" => "settings.advanced.modelUnload.options.min15",
        "hour1" => "settings.advanced.modelUnload.options.hour1",
        _ => "settings.advanced.modelUnload.options.sec15",
    }
}

// ---------------------------------------------------------------------------
// Acceleration (AccelerationSelector.tsx)
// ---------------------------------------------------------------------------

use guidy_core::managers::transcription::AvailableAccelerators;
use guidy_core::settings::TranscribeAcceleratorSetting;

/// One dropdown value encodes accelerator + device: `auto`, `cpu`, `gpu:<id>`.
pub fn encode_accelerator(
    accelerator: TranscribeAcceleratorSetting,
    gpu_device: Option<&str>,
) -> String {
    match (accelerator, gpu_device) {
        (TranscribeAcceleratorSetting::Cpu, _) => "cpu".into(),
        (TranscribeAcceleratorSetting::Gpu, Some(d)) => format!("gpu:{d}"),
        _ => "auto".into(),
    }
}

pub fn decode_accelerator(value: &str) -> (TranscribeAcceleratorSetting, Option<String>) {
    if value == "cpu" {
        (TranscribeAcceleratorSetting::Cpu, None)
    } else if let Some(id) = value.strip_prefix("gpu:") {
        (TranscribeAcceleratorSetting::Gpu, Some(id.to_string()))
    } else {
        (TranscribeAcceleratorSetting::Auto, None)
    }
}

/// `(value, label)` options: Auto, one per GPU ("name (VRAM)"), CPU.
pub fn accelerator_options(
    available: &AvailableAccelerators,
    auto_label: &str,
) -> Vec<(String, String)> {
    let mut v = Vec::new();
    if available.transcribe.iter().any(|t| t == "auto") {
        v.push(("auto".to_string(), auto_label.to_string()));
    }
    if available.transcribe.iter().any(|t| t == "gpu") {
        for d in &available.gpu_devices {
            let vram = if d.total_vram_mb >= 1024 {
                format!("{:.1} GB", d.total_vram_mb as f64 / 1024.0)
            } else {
                format!("{} MB", d.total_vram_mb)
            };
            v.push((format!("gpu:{}", d.id), format!("{} ({vram})", d.name)));
        }
    }
    if available.transcribe.iter().any(|t| t == "cpu") {
        v.push(("cpu".to_string(), "CPU".to_string()));
    }
    v
}

/// The displayed value: the encoded setting if offered, else the first option.
pub fn displayed_accelerator(current: &str, options: &[(String, String)]) -> Option<String> {
    if options.iter().any(|(v, _)| v == current) {
        Some(current.to_string())
    } else {
        options.first().map(|(v, _)| v.clone())
    }
}

// ---------------------------------------------------------------------------
// Post-processing (usePostProcessProviderState.ts)
// ---------------------------------------------------------------------------

pub const APPLE_PROVIDER_ID: &str = "apple_intelligence";
pub const CUSTOM_PROVIDER_ID: &str = "custom";

/// After switching provider, fetch its models only when it's configured
/// (custom: a base URL; others: an API key). Never for Apple Intelligence.
pub fn should_auto_fetch_models(provider: Option<ProviderRef>, api_key: &str) -> bool {
    match provider {
        // Built-in llama.cpp: the downloaded models, no key or URL needed.
        Some(ProviderRef::Builtin) => true,
        Some(ProviderRef::Configured(p)) => match p.id.as_str() {
            APPLE_PROVIDER_ID => false,
            CUSTOM_PROVIDER_ID => !p.base_url.trim().is_empty(),
            _ => !api_key.trim().is_empty(),
        },
        None => false,
    }
}

/// Fetched models (deduped, trimmed) plus the current model.
pub fn post_process_model_options(fetched: &[String], current: &str) -> Vec<String> {
    let mut seen = HashSetStr::new();
    let mut out = Vec::new();
    for v in fetched
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(current))
    {
        let t = v.trim();
        if !t.is_empty() && seen.insert(t.to_string()) {
            out.push(t.to_string());
        }
    }
    out
}

type HashSetStr = std::collections::HashSet<String>;

/// The prompt editor has unsaved changes.
pub fn prompt_is_dirty(
    saved_name: &str,
    saved_prompt: &str,
    draft_name: &str,
    draft_prompt: &str,
) -> bool {
    draft_name.trim() != saved_name || draft_prompt.trim() != saved_prompt.trim()
}

// ---------------------------------------------------------------------------
// Custom words (CustomWords.tsx)
// ---------------------------------------------------------------------------

pub const MAX_CUSTOM_WORD_LEN: usize = 50;

/// Strip `<>"'`, collapse whitespace, trim.
pub fn normalize_custom_word(word: &str) -> String {
    let stripped: String = word
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | '"' | '\''))
        .collect();
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddWordOutcome {
    Added(Vec<String>),
    Duplicate(String),
    Invalid,
}

pub fn add_custom_word(words: &[String], input: &str) -> AddWordOutcome {
    let w = normalize_custom_word(input);
    if w.is_empty() || w.chars().count() > MAX_CUSTOM_WORD_LEN {
        return AddWordOutcome::Invalid;
    }
    if words.contains(&w) {
        return AddWordOutcome::Duplicate(w);
    }
    let mut next = words.to_vec();
    next.push(w);
    AddWordOutcome::Added(next)
}

// ---------------------------------------------------------------------------
// Secure input banner (SecureInputWarning.tsx)
// ---------------------------------------------------------------------------

/// Learn-more link (Apple's guide to Secure Keyboard Entry).
pub const SECURE_INPUT_HELP_URL: &str =
    "https://support.apple.com/guide/terminal/use-secure-keyboard-entry-trml109/mac";

pub fn secure_input_impacted(s: &SecureInputStatus) -> bool {
    (s.sustained && (!s.degraded_bindings.is_empty() || !s.uncovered_bindings.is_empty()))
        || s.recorder_blocked
}

/// The banner text.
pub fn secure_input_message(s: &SecureInputStatus) -> String {
    use guidy_i18n::{t, t_args};
    let affected: BTreeSet<&String> = s
        .uncovered_bindings
        .iter()
        .chain(s.degraded_bindings.iter())
        .collect();
    let count = affected.len();
    let suffix = if count == 1 { "one" } else { "other" };
    let count_s = count.to_string();
    match (&s.culprit_name, count > 0) {
        (Some(name), true) => t_args(
            &format!("secureInput.blockedWithCulprit_{suffix}"),
            &[("name", name), ("count", &count_s)],
        ),
        (None, true) => t_args(
            &format!("secureInput.blockedNoCulprit_{suffix}"),
            &[("count", &count_s)],
        ),
        (Some(name), false) => t_args("secureInput.recorderBlockedWithCulprit", &[("name", name)]),
        (None, false) => t("secureInput.recorderBlockedNoCulprit"),
    }
}

// ---------------------------------------------------------------------------
// What's New (releaseNotes.ts)
// ---------------------------------------------------------------------------

pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim();
    let v = v
        .strip_prefix('v')
        .or_else(|| v.strip_prefix('V'))
        .unwrap_or(v);
    let mut parts = v.splitn(3, '.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let c_raw = parts.next()?;
    let digits: String = c_raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    let c = digits.parse().ok()?;
    Some((a, b, c))
}

/// The highest note ≤ current and > last seen (any note if last seen isn't a
/// valid version).
pub fn find_release_note_to_show<'a>(
    notes: &'a [(String, String)],
    current_version: &str,
    last_seen: &str,
) -> Option<&'a (String, String)> {
    let current = parse_version(current_version)?;
    let last = parse_version(last_seen);
    notes
        .iter()
        .filter(|(v, _)| parse_version(v).is_some_and(|nv| nv <= current))
        .filter(|(v, _)| match last {
            Some(l) => parse_version(v).is_some_and(|nv| nv > l),
            None => true,
        })
        .max_by_key(|(v, _)| parse_version(v))
}

pub fn find_latest_release_note(notes: &[(String, String)]) -> Option<&(String, String)> {
    notes
        .iter()
        .filter(|(v, _)| parse_version(v).is_some())
        .max_by_key(|(v, _)| parse_version(v))
}

/// Markdown link schemes the What's New renderer will open.
pub fn is_allowed_note_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

// ---------------------------------------------------------------------------
// Updater (portableInstaller.ts)
// ---------------------------------------------------------------------------

/// Where portable users go to download a new version when the manifest has no
/// installer for their platform.
pub const PORTABLE_RELEASES_URL: &str = env!("GUIDY_RELEASES_URL");

/// Pick the NSIS installer URL for the running target out of the update
/// manifest (`latest.json`); fall back to the releases page.
pub fn resolve_portable_installer_url(
    raw_json: Option<&serde_json::Value>,
    platform: &str,
    arch: &str,
) -> String {
    if platform != "windows" {
        return PORTABLE_RELEASES_URL.into();
    }
    raw_json
        .and_then(|j| j.get("platforms"))
        .filter(|p| p.is_object())
        .and_then(|p| p.get(format!("windows-{arch}-nsis")))
        .filter(|e| e.is_object())
        .and_then(|e| e.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| PORTABLE_RELEASES_URL.into())
}

// ---------------------------------------------------------------------------
// Live log viewer (LiveLogViewer.tsx)
// ---------------------------------------------------------------------------

pub const LOG_MAX_LINES: usize = 1000;
pub const LOG_FLUSH_MS: u64 = 250;
pub const LOG_PIN_THRESHOLD_PX: f32 = 24.0;

pub fn log_level_tag(level: u8) -> &'static str {
    match level {
        1 => "TRACE",
        2 => "DEBUG",
        3 => "INFO",
        4 => "WARN",
        5 => "ERROR",
        _ => "LOG",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub id: u64,
    pub level: u8,
    pub message: String,
    pub time: String,
}

/// Append a flushed batch, keeping the newest `LOG_MAX_LINES`.
pub fn append_log_lines(lines: &mut Vec<LogLine>, batch: Vec<LogLine>) {
    lines.extend(batch);
    if lines.len() > LOG_MAX_LINES {
        let excess = lines.len() - LOG_MAX_LINES;
        lines.drain(..excess);
    }
}

/// "HH:MM:SS LEVEL msg" per line.
pub fn format_logs_for_copy(lines: &[LogLine]) -> String {
    lines
        .iter()
        .map(|l| format!("{} {} {}", l.time, log_level_tag(l.level), l.message))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Keyboard diagnostic (KeyboardDiagnostic.tsx)
// ---------------------------------------------------------------------------

/// Verdict i18n key for a diagnostic report.
pub fn keyboard_verdict_key(
    r: &guidy_core::secure_input::KeyboardDiagnosticReport,
) -> &'static str {
    if r.secure_input_enabled && r.key_down == 0 {
        "settings.debug.keyboardDiagnostic.verdictBlocked"
    } else if !r.secure_input_enabled && r.key_down == 0 && r.flags_changed > 0 {
        "settings.debug.keyboardDiagnostic.verdictSuspicious"
    } else if r.key_down == 0 && r.flags_changed == 0 && r.mouse == 0 {
        "settings.debug.keyboardDiagnostic.verdictNoEvents"
    } else {
        "settings.debug.keyboardDiagnostic.verdictOk"
    }
}

// ---------------------------------------------------------------------------
// Settings-derived visibility
// ---------------------------------------------------------------------------

/// Sidebar sections in order with visibility (Sidebar.tsx).
pub fn sidebar_sections(s: Option<&AppSettings>) -> Vec<&'static str> {
    let mut v = vec!["general", "history", "models", "advanced"];
    if s.is_some_and(|s| s.post_process_enabled) {
        v.push("postprocessing");
    }
    v.push("usage");
    if s.is_some_and(|s| s.debug_mode) {
        v.push("debug");
    }
    v.push("about");
    v
}

/// Formats a seconds timestamp in the given locale (Intl long month,
/// 2-digit time).
pub fn format_history_date(timestamp_secs: i64, locale: &str) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_opt(timestamp_secs, 0).single() else {
        return timestamp_secs.to_string();
    };
    let loc = chrono_locale(locale);
    dt.format_localized("%B %-d, %Y %H:%M", loc).to_string()
}

/// The chrono locale for an app language code.
pub fn chrono_locale(code: &str) -> chrono::Locale {
    let tag = match code {
        "en" => "en_US",
        "zh" => "zh_CN",
        "zh-TW" => "zh_TW",
        "es" => "es_ES",
        "fr" => "fr_FR",
        "de" => "de_DE",
        "ja" => "ja_JP",
        "ko" => "ko_KR",
        "vi" => "vi_VN",
        "pl" => "pl_PL",
        "it" => "it_IT",
        "ru" => "ru_RU",
        "uk" => "uk_UA",
        "pt" => "pt_PT",
        "cs" => "cs_CZ",
        "tr" => "tr_TR",
        "ar" => "ar_SA",
        "he" => "he_IL",
        "sv" => "sv_SE",
        "bg" => "bg_BG",
        "nl" => "nl_NL",
        "ne" => "ne_NP",
        "hi" => "hi_IN",
        "da" => "da_DK",
        "ca" => "ca_ES",
        "id" => "id_ID",
        _ => "en_US",
    };
    chrono::Locale::try_from(tag).unwrap_or(chrono::Locale::en_US)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mic(supported: bool, access: PermissionAccess) -> WindowsMicrophonePermissionStatus {
        WindowsMicrophonePermissionStatus {
            supported,
            overall_access: access,
            device_access: access,
            app_access: access,
            desktop_app_access: access,
        }
    }

    #[test]
    fn new_users_go_welcome_permissions_setup() {
        let o = initial_step(&StartupFacts::default(), OsType::MacOs);
        assert_eq!(o.step, OnboardingStep::Welcome);
        assert!(!o.returning_user);
        assert_eq!(after_permissions(false, None).step, OnboardingStep::Setup);
        assert_eq!(
            step_back(OnboardingStep::Setup, false),
            Some(OnboardingStep::Permissions)
        );
        assert_eq!(
            step_back(OnboardingStep::Permissions, false),
            Some(OnboardingStep::Welcome)
        );
        assert_eq!(step_back(OnboardingStep::Welcome, false), None);
        // A returning user only sees permissions: no Back.
        assert_eq!(step_back(OnboardingStep::Permissions, true), None);
    }

    #[test]
    fn a_new_user_launched_hidden_sees_the_window() {
        let o = initial_step(&StartupFacts::default(), OsType::MacOs);
        assert!(o.reveal_window);
        let unreadable = StartupFacts {
            onboarding_completed: None,
            ..Default::default()
        };
        assert!(initial_step(&unreadable, OsType::Linux).reveal_window);
    }

    #[test]
    fn returning_users_skip_to_done_or_migration() {
        let mut f = StartupFacts {
            onboarding_completed: Some(true),
            mac_permissions_ok: Some(true),
            ..Default::default()
        };
        assert_eq!(initial_step(&f, OsType::MacOs).step, OnboardingStep::Done);
        f.migration_model_id = Some("old".into());
        let o = initial_step(&f, OsType::MacOs);
        assert_eq!(o.step, OnboardingStep::Migration);
        assert!(o.reveal_window);
        assert_eq!(
            after_permissions(true, Some("old")).step,
            OnboardingStep::Migration
        );
        assert_eq!(after_permissions(true, None).step, OnboardingStep::Done);
    }

    #[test]
    fn missing_permissions_send_returning_users_to_permissions() {
        let f = StartupFacts {
            onboarding_completed: Some(true),
            mac_permissions_ok: Some(false),
            ..Default::default()
        };
        let o = initial_step(&f, OsType::MacOs);
        assert_eq!(o.step, OnboardingStep::Permissions);
        assert!(o.returning_user && o.reveal_window);
        // A failed check proceeds to the app.
        let f = StartupFacts {
            onboarding_completed: Some(true),
            mac_permissions_ok: None,
            ..Default::default()
        };
        assert_eq!(initial_step(&f, OsType::MacOs).step, OnboardingStep::Done);
        let f = StartupFacts {
            onboarding_completed: Some(true),
            windows_mic: Some(mic(true, PermissionAccess::Denied)),
            ..Default::default()
        };
        assert_eq!(
            initial_step(&f, OsType::Windows).step,
            OnboardingStep::Permissions
        );
        let f = StartupFacts {
            onboarding_completed: Some(true),
            windows_mic: Some(mic(false, PermissionAccess::Denied)),
            ..Default::default()
        };
        assert_eq!(initial_step(&f, OsType::Windows).step, OnboardingStep::Done);
        // Linux never asks
        let f = StartupFacts {
            onboarding_completed: Some(true),
            mac_permissions_ok: Some(false),
            ..Default::default()
        };
        assert_eq!(initial_step(&f, OsType::Linux).step, OnboardingStep::Done);
    }

    fn values(v: &[OptionSpec]) -> Vec<&str> {
        v.iter().map(|o| o.value.as_str()).collect()
    }

    #[test]
    fn paste_method_matrix() {
        assert_eq!(
            values(&paste_method_options(OsType::MacOs, PasteMethod::CtrlV)),
            ["ctrl_v", "none"]
        );
        let mac_direct = paste_method_options(OsType::MacOs, PasteMethod::Direct);
        assert_eq!(values(&mac_direct), ["ctrl_v", "direct", "none"]);
        assert!(mac_direct[1].disabled);
        assert_eq!(
            values(&paste_method_options(OsType::Windows, PasteMethod::CtrlV)),
            ["ctrl_v", "direct", "none", "ctrl_shift_v", "shift_insert"]
        );
        assert_eq!(
            values(&paste_method_options(OsType::Linux, PasteMethod::Direct)),
            [
                "ctrl_v",
                "direct",
                "none",
                "ctrl_shift_v",
                "shift_insert",
                "external_script"
            ]
        );
        assert!(paste_method_options(OsType::MacOs, PasteMethod::CtrlV)[0]
            .label()
            .contains("Cmd"));
    }

    #[test]
    fn auto_submit_mapping() {
        assert_eq!(auto_submit_value(false, AutoSubmitKey::CtrlEnter), "off");
        assert_eq!(
            auto_submit_value(true, AutoSubmitKey::CmdEnter),
            "cmd_enter"
        );
        assert_eq!(
            auto_submit_writes("off", true),
            vec![("auto_submit", json!(false))]
        );
        assert_eq!(
            auto_submit_writes("ctrl_enter", false),
            vec![
                ("auto_submit_key", json!("ctrl_enter")),
                ("auto_submit", json!(true))
            ]
        );
        assert_eq!(
            auto_submit_writes("enter", true),
            vec![("auto_submit_key", json!("enter"))]
        );
        let mac = auto_submit_options(OsType::MacOs);
        assert_eq!(
            mac[3].label_key,
            "settings.advanced.autoSubmit.options.cmdEnter"
        );
        let lin = auto_submit_options(OsType::Linux);
        assert_eq!(
            lin[3].label_key,
            "settings.advanced.autoSubmit.options.superEnter"
        );
    }

    #[test]
    fn unload_options_gate_sec15_on_debug() {
        assert_eq!(model_unload_options(false).len(), 7);
        assert_eq!(model_unload_options(true).last().unwrap().value, "sec15");
    }

    #[test]
    fn custom_words_normalize_and_dedupe() {
        assert_eq!(
            normalize_custom_word("  <Guidy>   \"GPUI\"  "),
            "Guidy GPUI"
        );
        let words = vec!["Guidy".to_string()];
        assert_eq!(
            add_custom_word(&words, "Guidy"),
            AddWordOutcome::Duplicate("Guidy".into())
        );
        assert_eq!(add_custom_word(&words, "   "), AddWordOutcome::Invalid);
        assert_eq!(
            add_custom_word(&words, &"x".repeat(51)),
            AddWordOutcome::Invalid
        );
        assert_eq!(
            add_custom_word(&words, "Zed"),
            AddWordOutcome::Added(vec!["Guidy".into(), "Zed".into()])
        );
    }

    fn status() -> SecureInputStatus {
        SecureInputStatus {
            enabled: true,
            sustained: true,
            culprit_pid: None,
            culprit_name: None,
            fallback_active: true,
            covered_bindings: vec![],
            degraded_bindings: vec![],
            uncovered_bindings: vec![],
            recorder_blocked: false,
        }
    }

    #[test]
    fn secure_input_banner_rules() {
        let mut s = status();
        assert!(!secure_input_impacted(&s), "fully covered: silent");
        s.uncovered_bindings = vec!["transcribe".into()];
        assert!(secure_input_impacted(&s));
        assert_eq!(
            secure_input_message(&s),
            guidy_i18n::t_args("secureInput.blockedNoCulprit_one", &[("count", "1")])
        );
        s.degraded_bindings = vec!["cancel".into(), "transcribe".into()];
        s.culprit_name = Some("Terminal".into());
        let msg = secure_input_message(&s);
        assert!(msg.contains("Terminal"));
        assert!(msg.contains('2'));
        let mut s = status();
        s.sustained = false;
        s.recorder_blocked = true;
        assert!(secure_input_impacted(&s));
        assert_eq!(
            secure_input_message(&s),
            guidy_i18n::t("secureInput.recorderBlockedNoCulprit")
        );
    }

    fn notes() -> Vec<(String, String)> {
        ["0.9.0", "0.9.7", "1.0.0", "bogus"]
            .iter()
            .map(|v| (v.to_string(), format!("notes {v}")))
            .collect()
    }

    #[test]
    fn release_note_selection() {
        let n = notes();
        assert_eq!(
            find_release_note_to_show(&n, "0.9.8", "0.9.0").unwrap().0,
            "0.9.7"
        );
        assert_eq!(
            find_release_note_to_show(&n, "0.9.8", "").unwrap().0,
            "0.9.7"
        );
        assert!(find_release_note_to_show(&n, "0.9.8", "0.9.7").is_none());
        assert_eq!(
            find_release_note_to_show(&n, "v1.0.0", "0.9.7").unwrap().0,
            "1.0.0"
        );
        assert!(find_release_note_to_show(&n, "garbage", "").is_none());
        assert_eq!(find_latest_release_note(&n).unwrap().0, "1.0.0");
        assert_eq!(parse_version("1.2.3-beta"), Some((1, 2, 3)));
    }

    #[test]
    fn note_links_are_allowlisted() {
        assert!(is_allowed_note_link("https://example.com"));
        assert!(is_allowed_note_link("mailto:a@b.c"));
        assert!(!is_allowed_note_link("javascript:alert(1)"));
        assert!(!is_allowed_note_link("file:///etc/passwd"));
    }

    // Ported from portableInstaller.test.ts
    #[test]
    fn portable_installer_url_resolution() {
        let manifest = json!({"platforms": {
            "windows-x86_64-nsis": {"url": "https://x/Guidy_x64-setup.exe"},
            "windows-aarch64-nsis": {"url": "https://x/Guidy_arm64-setup.exe"}
        }});
        assert_eq!(
            resolve_portable_installer_url(Some(&manifest), "windows", "x86_64"),
            "https://x/Guidy_x64-setup.exe"
        );
        assert_eq!(
            resolve_portable_installer_url(Some(&manifest), "windows", "aarch64"),
            "https://x/Guidy_arm64-setup.exe"
        );
        assert_eq!(
            resolve_portable_installer_url(None, "windows", "x86_64"),
            PORTABLE_RELEASES_URL
        );
        assert_eq!(
            resolve_portable_installer_url(Some(&manifest), "windows", "x86"),
            PORTABLE_RELEASES_URL
        );
        assert_eq!(
            resolve_portable_installer_url(Some(&manifest), "macos", "aarch64"),
            PORTABLE_RELEASES_URL
        );
        assert_eq!(
            resolve_portable_installer_url(
                Some(&json!({"platforms": "nope"})),
                "windows",
                "x86_64"
            ),
            PORTABLE_RELEASES_URL
        );
    }

    #[test]
    fn log_buffer_caps_and_formats() {
        let mut lines = vec![];
        let batch: Vec<LogLine> = (0..1005)
            .map(|i| LogLine {
                id: i,
                level: 3,
                message: format!("m{i}"),
                time: "12:00:00".into(),
            })
            .collect();
        append_log_lines(&mut lines, batch);
        assert_eq!(lines.len(), LOG_MAX_LINES);
        assert_eq!(lines[0].message, "m5");
        assert_eq!(format_logs_for_copy(&lines[..1]), "12:00:00 INFO m5");
        assert_eq!(log_level_tag(9), "LOG");
    }

    #[test]
    fn sidebar_visibility() {
        let mut s = guidy_core::settings::get_default_settings();
        assert_eq!(
            sidebar_sections(Some(&s)),
            ["general", "history", "models", "advanced", "usage", "about"]
        );
        s.post_process_enabled = true;
        s.debug_mode = true;
        assert_eq!(
            sidebar_sections(Some(&s)),
            [
                "general",
                "history",
                "models",
                "advanced",
                "postprocessing",
                "usage",
                "debug",
                "about"
            ]
        );
    }

    #[test]
    fn history_dates_are_localized() {
        let en = format_history_date(0, "en");
        assert!(en.contains("1970") || en.contains("1969"));
        let de = format_history_date(1_700_000_000, "de");
        assert!(de.contains("November"));
        let fr = format_history_date(1_700_000_000, "fr");
        assert!(fr.contains("novembre"));
    }

    #[test]
    fn accelerator_encoding_round_trips() {
        use guidy_core::managers::transcription::GpuDeviceOption;
        assert_eq!(
            encode_accelerator(TranscribeAcceleratorSetting::Cpu, Some("x")),
            "cpu"
        );
        assert_eq!(
            encode_accelerator(TranscribeAcceleratorSetting::Gpu, Some("mtl0")),
            "gpu:mtl0"
        );
        assert_eq!(
            encode_accelerator(TranscribeAcceleratorSetting::Gpu, None),
            "auto"
        );
        assert_eq!(
            decode_accelerator("gpu:mtl0"),
            (TranscribeAcceleratorSetting::Gpu, Some("mtl0".into()))
        );
        assert_eq!(
            decode_accelerator("cpu"),
            (TranscribeAcceleratorSetting::Cpu, None)
        );
        assert_eq!(
            decode_accelerator("auto"),
            (TranscribeAcceleratorSetting::Auto, None)
        );
        let avail = AvailableAccelerators {
            transcribe: vec!["auto".into(), "cpu".into(), "gpu".into()],
            gpu_devices: vec![
                GpuDeviceOption {
                    id: "a".into(),
                    name: "Big".into(),
                    total_vram_mb: 24576,
                },
                GpuDeviceOption {
                    id: "b".into(),
                    name: "Small".into(),
                    total_vram_mb: 512,
                },
            ],
        };
        let opts = accelerator_options(&avail, "Auto");
        assert_eq!(
            opts,
            vec![
                ("auto".to_string(), "Auto".to_string()),
                ("gpu:a".to_string(), "Big (24.0 GB)".to_string()),
                ("gpu:b".to_string(), "Small (512 MB)".to_string()),
                ("cpu".to_string(), "CPU".to_string()),
            ]
        );
        assert_eq!(
            displayed_accelerator("gpu:zzz", &opts).as_deref(),
            Some("auto")
        );
        let cpu_only = AvailableAccelerators {
            transcribe: vec!["cpu".into()],
            gpu_devices: vec![],
        };
        assert_eq!(
            accelerator_options(&cpu_only, "Auto"),
            vec![("cpu".to_string(), "CPU".to_string())]
        );
    }

    #[test]
    fn post_process_rules() {
        let provider = |id: &str, base_url: &str| guidy_core::settings::PostProcessProvider {
            id: id.into(),
            label: id.into(),
            base_url: base_url.into(),
            allow_base_url_edit: false,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: Default::default(),
        };
        let fetch = |p: &guidy_core::settings::PostProcessProvider, key: &str| {
            should_auto_fetch_models(Some(ProviderRef::Configured(p)), key)
        };
        assert!(!fetch(&provider("apple_intelligence", "x"), "k"));
        assert!(fetch(&provider("custom", "http://x"), ""));
        assert!(!fetch(&provider("custom", "  "), "key"));
        assert!(fetch(&provider("openai", ""), "sk"));
        assert!(!fetch(&provider("openai", ""), " "));
        // The built-in runtime needs neither a key nor a URL.
        assert!(should_auto_fetch_models(Some(ProviderRef::Builtin), ""));
        assert!(!should_auto_fetch_models(None, "sk"));
        assert_eq!(
            post_process_model_options(&[" a ".into(), "b".into(), "a".into()], "c"),
            vec!["a", "b", "c"]
        );
        assert_eq!(post_process_model_options(&["a".into()], "a"), vec!["a"]);
        assert!(!prompt_is_dirty("n", "p", " n ", "p "));
        assert!(prompt_is_dirty("n", "p", "n2", "p"));
    }

    #[test]
    fn keyboard_verdicts() {
        use guidy_core::secure_input::KeyboardDiagnosticReport;
        let r = |sec: bool, kd: u32, fc: u32, m: u32| KeyboardDiagnosticReport {
            secure_input_enabled: sec,
            culprit_pid: None,
            culprit_name: None,
            key_down: kd,
            key_up: 0,
            flags_changed: fc,
            mouse: m,
            duration_ms: 10_000,
        };
        assert!(keyboard_verdict_key(&r(true, 0, 3, 0)).ends_with("verdictBlocked"));
        assert!(keyboard_verdict_key(&r(false, 0, 3, 0)).ends_with("verdictSuspicious"));
        assert!(keyboard_verdict_key(&r(false, 0, 0, 0)).ends_with("verdictNoEvents"));
        assert!(keyboard_verdict_key(&r(false, 5, 0, 0)).ends_with("verdictOk"));
    }
}
