//! Who a meeting app says is speaking, read through Accessibility about once a
//! second while recording. Optional twice over: it needs the Accessibility
//! grant, and the `speaker_names_from_apps` setting, off by default, since the
//! apps' accessibility trees change often and these patterns haven't been
//! checked on real calls (`docs/plans/meetings.md`, Phase 5). Without either
//! nothing is read, and nothing changes.
//!
//! Names go to `<id>.speakers.jsonl` beside the tracks, local only, and are
//! never logged; [`crate::meetings::processing::speaker_hints`] reads them back
//! as a suggestion the user can edit.

/// The longest name believed.
const MAX_NAME_CHARS: usize = 60;

/// The name in an accessibility label saying who's speaking: `Talking: Ann`,
/// `Speaking: Ann`, `Ann is speaking`, `Ann is talking`, `Ann, speaking`,
/// `Ann (speaking)`. `None` for anything else, like a "Speaker view" button.
pub fn parse_speaker_label(label: &str) -> Option<String> {
    let label = label.trim();
    let lower = label.to_ascii_lowercase();
    let name = ["talking:", "speaking:"]
        .iter()
        .find(|p| lower.starts_with(**p))
        .and_then(|p| label.get(p.len()..))
        .or_else(|| {
            [" is speaking", " is talking", ", speaking", " (speaking)"]
                .iter()
                .find(|s| lower.ends_with(**s))
                .and_then(|s| label.get(..label.len() - s.len()))
        })?
        .trim();
    let lower = name.to_ascii_lowercase();
    let believable = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.contains(':')
        && !name.chars().any(|c| {
            // Line breaks and the like, and characters that reorder display.
            c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        && !["you", "no one", "nobody", "someone", "everyone"].contains(&lower.as_str())
        && !lower.contains("speak");
    believable.then(|| name.to_string())
}

/// The call apps to read, among those on the microphone. A browser only with a
/// Meet window: its other pages say nothing of this kind.
#[cfg(any(target_os = "macos", test))]
fn readable(app_id: &str, titles: impl FnOnce() -> Vec<String>) -> bool {
    match app_id {
        "us.zoom.xos" | "com.microsoft.teams2" => true,
        "com.google.Chrome" => titles()
            .iter()
            .any(|t| crate::meetings::calls::is_meet_title(t)),
        _ => false,
    }
}

/// Who the call on the microphone says is speaking now, if it says. Bounded
/// well under a second: this runs on the recorder's health tick.
pub fn active_speaker() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use super::macos;
        super::input_apps_from(&macos::audio_input_processes())
            .iter()
            .filter(|a| readable(&a.canonical, || macos::window_titles(&a.canonical)))
            .find_map(|a| {
                macos::speaker_labels(&a.canonical)
                    .iter()
                    .find_map(|label| parse_speaker_label(label))
            })
    }
    #[cfg(not(target_os = "macos"))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_that_name_a_speaker() {
        for label in [
            "Talking: Ann Lee",
            "Speaking: Ann Lee",
            "Ann Lee is speaking",
            "Ann Lee is talking",
            "Ann Lee, speaking",
            "Ann Lee (speaking)",
            "  talking:  Ann Lee ",
        ] {
            assert_eq!(
                parse_speaker_label(label).as_deref(),
                Some("Ann Lee"),
                "{label}"
            );
        }
        // Not ASCII: slicing stays on the labels' own bytes.
        assert_eq!(
            parse_speaker_label("Zoë Ñandú is speaking").as_deref(),
            Some("Zoë Ñandú")
        );
    }

    #[test]
    fn labels_that_do_not() {
        for label in [
            "",
            "Speaker view",
            "Speaking",
            "Talking:",
            "Speaker 1 is speaking",
            "You are speaking",
            "You is speaking",
            "No one is speaking",
            "Mute microphone",
            "Stop talking: a guide: part 2",
        ] {
            assert_eq!(parse_speaker_label(label), None, "{label}");
        }
        for label in [
            "An\nn is speaking",
            "An\u{0}n is speaking",
            "An\u{202E}n is speaking",
        ] {
            assert_eq!(parse_speaker_label(label), None, "{label:?}");
        }
        let long = format!("{} is speaking", "x".repeat(MAX_NAME_CHARS + 1));
        assert_eq!(parse_speaker_label(&long), None);
    }

    #[test]
    fn only_call_apps_are_read_and_a_browser_only_on_meet() {
        assert!(readable("us.zoom.xos", Vec::new));
        assert!(readable("com.microsoft.teams2", Vec::new));
        assert!(!readable("com.tinyspeck.slackmacgap", Vec::new));
        assert!(!readable("com.google.Chrome", Vec::new));
        assert!(!readable("com.google.Chrome", || vec!["Docs".to_string()]));
        assert!(readable("com.google.Chrome", || {
            vec!["Meet - abc-defg-hij".to_string()]
        }));
    }
}
