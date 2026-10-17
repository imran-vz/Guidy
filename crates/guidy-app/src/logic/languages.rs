//! Recognition languages.

/// `(code, English label)`; `auto` first.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Auto Detect"),
    ("en", "English"),
    ("zh", "Chinese (Mandarin)"),
    ("yue", "Cantonese"),
    ("de", "German"),
    ("es", "Spanish"),
    ("ru", "Russian"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("ja", "Japanese"),
    ("pt", "Portuguese"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("ca", "Catalan"),
    ("nl", "Dutch"),
    ("ar", "Arabic"),
    ("sv", "Swedish"),
    ("it", "Italian"),
    ("id", "Indonesian"),
    ("hi", "Hindi"),
    ("fi", "Finnish"),
    ("vi", "Vietnamese"),
    ("he", "Hebrew"),
    ("uk", "Ukrainian"),
    ("el", "Greek"),
    ("ms", "Malay"),
    ("cs", "Czech"),
    ("ro", "Romanian"),
    ("da", "Danish"),
    ("hu", "Hungarian"),
    ("ta", "Tamil"),
    ("no", "Norwegian"),
    ("th", "Thai"),
    ("ur", "Urdu"),
    ("hr", "Croatian"),
    ("bg", "Bulgarian"),
    ("lt", "Lithuanian"),
    ("la", "Latin"),
    ("mi", "Maori"),
    ("ml", "Malayalam"),
    ("cy", "Welsh"),
    ("sk", "Slovak"),
    ("te", "Telugu"),
    ("fa", "Persian"),
    ("lv", "Latvian"),
    ("bn", "Bengali"),
    ("sr", "Serbian"),
    ("az", "Azerbaijani"),
    ("sl", "Slovenian"),
    ("kn", "Kannada"),
    ("et", "Estonian"),
    ("mk", "Macedonian"),
    ("br", "Breton"),
    ("eu", "Basque"),
    ("is", "Icelandic"),
    ("ga", "Irish"),
    ("hy", "Armenian"),
    ("ne", "Nepali"),
    ("mn", "Mongolian"),
    ("bs", "Bosnian"),
    ("kk", "Kazakh"),
    ("sq", "Albanian"),
    ("sw", "Swahili"),
    ("gl", "Galician"),
    ("mr", "Marathi"),
    ("pa", "Punjabi"),
    ("si", "Sinhala"),
    ("km", "Khmer"),
    ("sn", "Shona"),
    ("yo", "Yoruba"),
    ("so", "Somali"),
    ("af", "Afrikaans"),
    ("oc", "Occitan"),
    ("ka", "Georgian"),
    ("be", "Belarusian"),
    ("tg", "Tajik"),
    ("sd", "Sindhi"),
    ("gu", "Gujarati"),
    ("am", "Amharic"),
    ("yi", "Yiddish"),
    ("lo", "Lao"),
    ("uz", "Uzbek"),
    ("fo", "Faroese"),
    ("ht", "Haitian Creole"),
    ("ps", "Pashto"),
    ("tk", "Turkmen"),
    ("nn", "Nynorsk"),
    ("mt", "Maltese"),
    ("sa", "Sanskrit"),
    ("lb", "Luxembourgish"),
    ("my", "Myanmar"),
    ("bo", "Tibetan"),
    ("tl", "Filipino (Tagalog)"),
    ("mg", "Malagasy"),
    ("as", "Assamese"),
    ("tt", "Tatar"),
    ("haw", "Hawaiian"),
    ("ln", "Lingala"),
    ("ha", "Hausa"),
    ("ba", "Bashkir"),
    ("jw", "Javanese"),
    ("su", "Sundanese"),
];

const LANGUAGE_ALIASES: &[(&str, &str)] = &[("nb", "no"), ("fil", "tl")];

/// Model-capability filter list (everything but `auto`).
pub fn capability_languages() -> impl Iterator<Item = (&'static str, &'static str)> {
    LANGUAGES
        .iter()
        .copied()
        .filter(|(code, _)| *code != "auto")
}

/// Collapse a tag to the canonical recognition intent: drop subtags
/// ("en-US" → "en") and map aliases (nb → no, fil → tl).
pub fn recognition_language(code: &str) -> String {
    let base = code.split('-').next().unwrap_or(code);
    LANGUAGE_ALIASES
        .iter()
        .find(|(from, _)| *from == base)
        .map(|(_, to)| to.to_string())
        .unwrap_or_else(|| base.to_string())
}

pub fn supports_language_code(supported: &[String], code: &str) -> bool {
    let wanted = recognition_language(code);
    supported.iter().any(|s| recognition_language(s) == wanted)
}

pub fn unique_capability_languages(supported: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    supported
        .iter()
        .map(|s| recognition_language(s))
        .filter(|c| seen.insert(c.clone()))
        .collect()
}

pub fn language_label(code: &str) -> Option<&'static str> {
    let find = |c: &str| LANGUAGES.iter().find(|(v, _)| *v == c).map(|(_, l)| *l);
    find(code).or_else(|| find(&recognition_language(code)))
}

/// The language actually used for the current model (mirrors
/// `effective_language` in the backend's model manager).
pub fn effective_language(intent: &str, supported: &[String], supports_detection: bool) -> String {
    if supported.is_empty() {
        return recognition_language(intent);
    }
    if intent != "auto" && supports_language_code(supported, intent) {
        return recognition_language(intent);
    }
    if supports_detection {
        return "auto".into();
    }
    if supports_language_code(supported, "en") {
        return "en".into();
    }
    recognition_language(&supported[0])
}

/// Languages offered by the selector for a model.
pub fn available_languages(
    supported: &[String],
    supports_detection: bool,
) -> Vec<(&'static str, &'static str)> {
    if supported.is_empty() {
        return LANGUAGES.to_vec();
    }
    LANGUAGES
        .iter()
        .copied()
        .filter(|(code, _)| {
            if *code == "auto" {
                supports_detection
            } else {
                supports_language_code(supported, code)
            }
        })
        .collect()
}

/// Case-insensitive label search.
pub fn filter_languages(
    langs: &[(&'static str, &'static str)],
    query: &str,
) -> Vec<(&'static str, &'static str)> {
    let q = query.to_lowercase();
    langs
        .iter()
        .copied()
        .filter(|(_, label)| label.to_lowercase().contains(&q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn has_all_languages() {
        assert_eq!(LANGUAGES.len(), 102);
        assert_eq!(LANGUAGES[0], ("auto", "Auto Detect"));
        assert_eq!(capability_languages().count(), 101);
    }

    #[test]
    fn recognition_language_drops_subtags_and_maps_aliases() {
        assert_eq!(recognition_language("en-US"), "en");
        assert_eq!(recognition_language("zh-Hant"), "zh");
        assert_eq!(recognition_language("nb"), "no");
        assert_eq!(recognition_language("fil"), "tl");
        assert_eq!(recognition_language("nn"), "nn");
    }

    #[test]
    fn effective_language_resolution() {
        let s = v(&["en-US", "de", "nb"]);
        assert_eq!(effective_language("de", &s, true), "de");
        assert_eq!(effective_language("no", &s, false), "no");
        assert_eq!(effective_language("fr", &s, true), "auto");
        assert_eq!(effective_language("fr", &s, false), "en");
        assert_eq!(effective_language("auto", &v(&["ja", "ko"]), false), "ja");
        assert_eq!(effective_language("auto", &[], false), "auto");
    }

    #[test]
    fn available_languages_gate_auto_on_detection() {
        let s = v(&["en", "de"]);
        let with = available_languages(&s, true);
        assert_eq!(
            with.iter().map(|l| l.0).collect::<Vec<_>>(),
            vec!["auto", "en", "de"]
        );
        let without = available_languages(&s, false);
        assert!(!without.iter().any(|l| l.0 == "auto"));
        assert_eq!(available_languages(&[], false).len(), 102);
    }

    #[test]
    fn labels_and_filtering() {
        let no_label = LANGUAGES.iter().find(|l| l.0 == "no").unwrap().1;
        assert_eq!(language_label("nb"), Some(no_label));
        assert_eq!(filter_languages(LANGUAGES, "GERM"), vec![("de", "German")]);
        assert_eq!(
            unique_capability_languages(&v(&["en-US", "en-GB", "de"])),
            v(&["en", "de"])
        );
    }
}
