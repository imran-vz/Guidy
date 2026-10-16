//! Shortcut key naming and display (port of Handy's `lib/utils/keyboard.ts`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsType {
    MacOs,
    Windows,
    Linux,
    Unknown,
}

impl OsType {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            OsType::MacOs
        } else if cfg!(target_os = "windows") {
            OsType::Windows
        } else if cfg!(target_os = "linux") {
            OsType::Linux
        } else {
            OsType::Unknown
        }
    }
}

/// A key event as the browser described it: `code` (physical key, e.g.
/// `KeyA`) and `key` (produced value, e.g. `a`).
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyInput<'a> {
    pub code: Option<&'a str>,
    pub key: Option<&'a str>,
}

fn modifier_name(base: &str, os: OsType) -> &'static str {
    match base {
        "shift" => "shift",
        "ctrl" => "ctrl",
        "alt" => {
            if os == OsType::MacOs {
                "option"
            } else {
                "alt"
            }
        }
        "meta" => {
            if os == OsType::MacOs {
                "command"
            } else {
                "super"
            }
        }
        _ => "",
    }
}

/// Extract a consistent key name (Handy's `getKeyName`).
pub fn get_key_name(e: KeyInput<'_>, os: OsType) -> String {
    if let Some(code) = e.code.filter(|c| !c.is_empty()) {
        // Function keys (F1-F24)
        if code.len() > 1 && code.starts_with('F') && code[1..].chars().all(|c| c.is_ascii_digit())
        {
            return code.to_lowercase();
        }
        // KeyA -> a
        if code.len() == 4 && code.starts_with("Key") && code.as_bytes()[3].is_ascii_uppercase() {
            return code[3..].to_lowercase();
        }
        // Digit0 -> 0
        if code.len() == 6 && code.starts_with("Digit") && code.as_bytes()[5].is_ascii_digit() {
            return code[5..].to_string();
        }
        // Numpad0 -> numpad 0
        if code.len() == 7 && code.starts_with("Numpad") && code.as_bytes()[6].is_ascii_digit() {
            return format!("numpad {}", &code[6..]);
        }
        let mapped = match code {
            "ShiftLeft" | "ShiftRight" => Some(modifier_name("shift", os)),
            "ControlLeft" | "ControlRight" => Some(modifier_name("ctrl", os)),
            "AltLeft" | "AltRight" => Some(modifier_name("alt", os)),
            "MetaLeft" | "MetaRight" | "OSLeft" | "OSRight" => Some(modifier_name("meta", os)),
            "CapsLock" => Some("capslock"),
            "Tab" => Some("tab"),
            "Enter" => Some("enter"),
            "Space" => Some("space"),
            "Backspace" => Some("backspace"),
            "Delete" => Some("delete"),
            "Escape" => Some("esc"),
            "ArrowUp" => Some("up"),
            "ArrowDown" => Some("down"),
            "ArrowLeft" => Some("left"),
            "ArrowRight" => Some("right"),
            "Home" => Some("home"),
            "End" => Some("end"),
            "PageUp" => Some("pageup"),
            "PageDown" => Some("pagedown"),
            "Insert" => Some("insert"),
            "PrintScreen" => Some("printscreen"),
            "ScrollLock" => Some("scrolllock"),
            "Pause" => Some("pause"),
            "ContextMenu" => Some("menu"),
            "NumpadMultiply" => Some("numpad *"),
            "NumpadAdd" => Some("numpad +"),
            "NumpadSubtract" => Some("numpad -"),
            "NumpadDecimal" => Some("numpad ."),
            "NumpadDivide" => Some("numpad /"),
            "NumLock" => Some("numlock"),
            "Semicolon" => Some(";"),
            "Equal" => Some("="),
            "Comma" => Some(","),
            "Minus" => Some("-"),
            "Period" => Some("."),
            "Slash" => Some("/"),
            "Backquote" => Some("`"),
            "BracketLeft" => Some("["),
            "Backslash" => Some("\\"),
            "BracketRight" => Some("]"),
            "Quote" => Some("'"),
            _ => None,
        };
        if let Some(m) = mapped {
            return m.to_string();
        }
        // Parser names mirror KeyboardEvent.code without spaces.
        return code.to_lowercase();
    }

    if let Some(key) = e.key.filter(|k| !k.is_empty()) {
        let mapped = match key {
            "Control" => Some("ctrl"),
            "Alt" => Some(if os == OsType::MacOs { "option" } else { "alt" }),
            "Shift" => Some("shift"),
            "Meta" | "OS" => Some(match os {
                OsType::MacOs => "command",
                OsType::Windows => "win",
                _ => "super",
            }),
            "CapsLock" => Some("capslock"),
            "ArrowUp" => Some("up"),
            "ArrowDown" => Some("down"),
            "ArrowLeft" => Some("left"),
            "ArrowRight" => Some("right"),
            "Escape" => Some("esc"),
            " " => Some("space"),
            _ => None,
        };
        return mapped
            .map(str::to_string)
            .unwrap_or_else(|| key.to_lowercase());
    }

    "unknown-0".to_string()
}

/// Map a GPUI keystroke key name onto Handy's shortcut token.
pub fn gpui_key_name(key: &str, os: OsType) -> String {
    match key {
        "escape" => "esc".into(),
        "space" | "enter" | "tab" | "backspace" | "delete" | "up" | "down" | "left" | "right"
        | "home" | "end" | "pageup" | "pagedown" | "insert" => key.into(),
        "shift" | "control" | "ctrl" => {
            if key == "shift" {
                "shift".into()
            } else {
                "ctrl".into()
            }
        }
        "alt" => modifier_name("alt", os).into(),
        "cmd" | "platform" | "super" | "win" => modifier_name("meta", os).into(),
        other => other.to_lowercase(),
    }
}

fn compound_display(key: &str) -> Option<&'static str> {
    Some(match key {
        "capslock" => "Caps Lock",
        "numlock" => "Num Lock",
        "pageup" => "Page Up",
        "pagedown" => "Page Down",
        "printscreen" => "Print Screen",
        "scrolllock" => "Scroll Lock",
        _ => return None,
    })
}

fn capitalize_key(key: &str) -> String {
    if key == "fn" {
        return "fn".into();
    }
    if key.len() > 1 && key.starts_with('f') && key[1..].chars().all(|c| c.is_ascii_digit()) {
        return key.to_uppercase();
    }
    if key.chars().count() == 1 {
        return key.to_uppercase();
    }
    if let Some(d) = compound_display(key) {
        return d.into();
    }
    // Capitalize the first letter of each word.
    let mut out = String::with_capacity(key.len());
    let mut at_word_start = true;
    for c in key.chars() {
        if at_word_start && c.is_alphanumeric() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        at_word_start = !(c.is_alphanumeric() || c == '_');
    }
    out
}

fn format_key_part(part: &str) -> String {
    let trimmed = part.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(name) = trimmed.strip_suffix("_left") {
        return format!("Left {}", capitalize_key(name));
    }
    if let Some(name) = trimmed.strip_suffix("_right") {
        return format!("Right {}", capitalize_key(name));
    }
    capitalize_key(trimmed)
}

/// "option_left+shift+space" → "Left Option + Shift + Space".
pub fn format_key_combination(combination: &str) -> String {
    if combination.is_empty() {
        return String::new();
    }
    combination
        .split('+')
        .map(format_key_part)
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Strip a "left "/"right " prefix from a modifier name.
pub fn normalize_key(key: &str) -> &str {
    for prefix in ["left ", "right "] {
        if let Some(rest) = key.strip_prefix(prefix) {
            if !rest.contains(' ') {
                return rest;
            }
        }
    }
    key
}

/// The modifier names GlobalShortcutInput sorts to the front.
const RECORDER_MODIFIERS: &[&str] = &[
    "ctrl", "control", "shift", "alt", "option", "meta", "command", "cmd", "super", "win",
    "windows",
];

pub fn is_modifier(key: &str) -> bool {
    RECORDER_MODIFIERS.contains(&normalize_key(&key.to_lowercase()))
}

/// Join recorded keys into a binding string: modifiers first (in recording
/// order), then the rest (Handy's GlobalShortcutInput).
pub fn combine_recorded_keys(keys: &[String]) -> String {
    let mut modifiers: Vec<&str> = Vec::new();
    let mut others: Vec<&str> = Vec::new();
    for k in keys {
        if is_modifier(k) {
            modifiers.push(k);
        } else {
            others.push(k);
        }
    }
    modifiers
        .into_iter()
        .chain(others)
        .collect::<Vec<_>>()
        .join("+")
}

/// System recorder (GlobalShortcutInput): keys accumulate while held; the
/// shortcut commits when every key has been released.
#[derive(Clone, Debug, Default)]
pub struct SystemRecorder {
    pub pressed: Vec<String>,
    pub recorded: Vec<String>,
}

impl SystemRecorder {
    /// `repeat` key events are ignored.
    pub fn key_down(&mut self, key: &str, repeat: bool) {
        if repeat {
            return;
        }
        let key = normalize_key(key).to_string();
        if !self.pressed.contains(&key) {
            self.pressed.push(key.clone());
            if !self.recorded.contains(&key) {
                self.recorded.push(key);
            }
        }
    }

    /// Returns the binding to commit once nothing is held.
    pub fn key_up(&mut self, key: &str) -> Option<String> {
        let key = normalize_key(key);
        self.pressed.retain(|k| k != key);
        if self.pressed.is_empty() && !self.recorded.is_empty() {
            let combo = combine_recorded_keys(&self.recorded);
            self.recorded.clear();
            return Some(combo);
        }
        None
    }

    /// Display text while recording.
    pub fn display(&self) -> Option<String> {
        (!self.recorded.is_empty()).then(|| format_key_combination(&self.recorded.join("+")))
    }
}

/// Map GPUI modifier flags to recorder key names.
pub fn modifier_names(
    control: bool,
    alt: bool,
    shift: bool,
    platform: bool,
    function: bool,
    os: OsType,
) -> Vec<&'static str> {
    let mut v = Vec::new();
    if control {
        v.push("ctrl");
    }
    if alt {
        v.push(modifier_name("alt", os));
    }
    if shift {
        v.push("shift");
    }
    if platform {
        v.push(modifier_name("meta", os));
    }
    if function {
        v.push("fn");
    }
    v
}

/// Backend key event (handy-keys) as the recorder sees it.
#[derive(Clone, Debug)]
pub struct HandyKeysInput<'a> {
    pub modifiers: &'a [String],
    pub key: Option<&'a str>,
    pub is_key_down: bool,
    pub hotkey_string: &'a str,
}

/// handy-keys recorder (HandyKeysShortcutInput): a keyed combo commits on
/// its key's release; a modifier-only shortcut only once every modifier is
/// released and no main key ever went down (issue #1578).
#[derive(Clone, Debug, Default)]
pub struct HandyKeysRecorder {
    pub keyed: String,
    pub modifier_only: String,
    pub current: String,
}

impl HandyKeysRecorder {
    pub fn on_event(&mut self, e: &HandyKeysInput<'_>) -> Option<String> {
        if e.is_key_down && !e.hotkey_string.is_empty() {
            if e.key.is_some() {
                self.keyed = e.hotkey_string.to_string();
            } else {
                self.modifier_only = e.hotkey_string.to_string();
            }
            self.current = e.hotkey_string.to_string();
            None
        } else if !e.is_key_down && e.key.is_some() {
            let commit = if self.keyed.is_empty() {
                e.hotkey_string.to_string()
            } else {
                self.keyed.clone()
            };
            (!commit.is_empty()).then_some(commit)
        } else if !e.is_key_down
            && e.key.is_none()
            && e.modifiers.is_empty()
            && self.keyed.is_empty()
            && !self.modifier_only.is_empty()
        {
            Some(self.modifier_only.clone())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(c: &str) -> KeyInput<'_> {
        KeyInput {
            code: Some(c),
            key: None,
        }
    }

    // Ported from keyboard.test.ts
    #[test]
    fn compound_keys_store_compact_and_display_spaced() {
        for (c, stored, displayed) in [
            ("ScrollLock", "scrolllock", "Scroll Lock"),
            ("CapsLock", "capslock", "Caps Lock"),
            ("NumLock", "numlock", "Num Lock"),
            ("PageUp", "pageup", "Page Up"),
            ("PageDown", "pagedown", "Page Down"),
            ("PrintScreen", "printscreen", "Print Screen"),
        ] {
            assert_eq!(get_key_name(code(c), OsType::Unknown), stored);
            assert_eq!(format_key_combination(stored), displayed);
        }
    }

    #[test]
    fn key_fallback_and_unknown_codes() {
        assert_eq!(
            get_key_name(
                KeyInput {
                    code: None,
                    key: Some("CapsLock")
                },
                OsType::Unknown
            ),
            "capslock"
        );
        assert_eq!(
            get_key_name(code("AudioVolumeUp"), OsType::Unknown),
            "audiovolumeup"
        );
    }

    #[test]
    fn os_specific_modifiers() {
        assert_eq!(get_key_name(code("AltLeft"), OsType::MacOs), "option");
        assert_eq!(get_key_name(code("AltLeft"), OsType::Linux), "alt");
        assert_eq!(get_key_name(code("MetaRight"), OsType::MacOs), "command");
        assert_eq!(get_key_name(code("MetaRight"), OsType::Windows), "super");
        let meta = KeyInput {
            code: None,
            key: Some("Meta"),
        };
        assert_eq!(get_key_name(meta, OsType::Windows), "win");
    }

    #[test]
    fn letters_digits_function_and_numpad() {
        assert_eq!(get_key_name(code("KeyA"), OsType::Unknown), "a");
        assert_eq!(get_key_name(code("Digit7"), OsType::Unknown), "7");
        assert_eq!(get_key_name(code("F13"), OsType::Unknown), "f13");
        assert_eq!(get_key_name(code("Numpad4"), OsType::Unknown), "numpad 4");
        assert_eq!(get_key_name(code("Slash"), OsType::Unknown), "/");
        assert_eq!(get_key_name(code("Escape"), OsType::Unknown), "esc");
    }

    #[test]
    fn formats_sides_function_keys_and_fn() {
        assert_eq!(
            format_key_combination("option_left+shift+space"),
            "Left Option + Shift + Space"
        );
        assert_eq!(format_key_combination("fn+f5"), "fn + F5");
        assert_eq!(format_key_combination("numpad 4"), "Numpad 4");
        assert_eq!(format_key_combination(""), "");
    }

    #[test]
    fn normalizes_and_orders_modifiers() {
        assert_eq!(normalize_key("left shift"), "shift");
        assert_eq!(normalize_key("right option"), "option");
        assert_eq!(normalize_key("space"), "space");
        let keys: Vec<String> = ["space", "shift", "ctrl"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(combine_recorded_keys(&keys), "shift+ctrl+space");
    }

    #[test]
    fn gpui_names_map_to_tokens() {
        assert_eq!(gpui_key_name("escape", OsType::Linux), "esc");
        assert_eq!(gpui_key_name("alt", OsType::MacOs), "option");
        assert_eq!(gpui_key_name("cmd", OsType::MacOs), "command");
        assert_eq!(gpui_key_name("A", OsType::Linux), "a");
    }

    #[test]
    fn system_recorder_commits_when_all_keys_are_released() {
        let mut r = SystemRecorder::default();
        r.key_down("space", false);
        r.key_down("shift", false);
        r.key_down("shift", true); // auto-repeat ignored
        assert_eq!(r.display().as_deref(), Some("Space + Shift"));
        assert_eq!(r.key_up("space"), None, "shift still held");
        assert_eq!(r.key_up("shift").as_deref(), Some("shift+space"));
        assert!(r.recorded.is_empty());
    }

    #[test]
    fn system_recorder_normalizes_sided_names() {
        let mut r = SystemRecorder::default();
        r.key_down("left ctrl", false);
        r.key_down("a", false);
        r.key_up("a");
        assert_eq!(r.key_up("left ctrl").as_deref(), Some("ctrl+a"));
    }

    fn ev<'a>(
        mods: &'a [String],
        key: Option<&'a str>,
        down: bool,
        s: &'a str,
    ) -> HandyKeysInput<'a> {
        HandyKeysInput {
            modifiers: mods,
            key,
            is_key_down: down,
            hotkey_string: s,
        }
    }

    #[test]
    fn handy_keys_commit_on_main_key_release() {
        let opt = vec!["option".to_string()];
        let none: Vec<String> = vec![];
        let mut r = HandyKeysRecorder::default();
        assert_eq!(r.on_event(&ev(&opt, None, true, "option")), None);
        assert_eq!(
            r.on_event(&ev(&opt, Some("space"), true, "option+space")),
            None
        );
        assert_eq!(r.current, "option+space");
        assert_eq!(
            r.on_event(&ev(&opt, Some("space"), false, "option+space"))
                .as_deref(),
            Some("option+space")
        );
        // modifier release afterwards doesn't commit a modifier-only binding
        let mut r = HandyKeysRecorder::default();
        r.on_event(&ev(&opt, None, true, "option"));
        r.on_event(&ev(&opt, Some("space"), true, "option+space"));
        assert_eq!(r.on_event(&ev(&none, None, false, "")), None);
    }

    #[test]
    fn handy_keys_modifier_only_after_full_release() {
        let opt = vec!["option".to_string()];
        let both = vec!["option".to_string(), "shift".to_string()];
        let none: Vec<String> = vec![];
        let mut r = HandyKeysRecorder::default();
        r.on_event(&ev(&opt, None, true, "option"));
        r.on_event(&ev(&both, None, true, "option+shift"));
        assert_eq!(
            r.on_event(&ev(&opt, None, false, "option")),
            None,
            "shift still held"
        );
        assert_eq!(
            r.on_event(&ev(&none, None, false, "")).as_deref(),
            Some("option+shift")
        );
    }

    #[test]
    fn gpui_modifier_mapping() {
        assert_eq!(
            modifier_names(true, true, false, true, false, OsType::MacOs),
            ["ctrl", "option", "command"]
        );
        assert_eq!(
            modifier_names(false, true, false, true, true, OsType::Linux),
            ["alt", "super", "fn"]
        );
    }
}
