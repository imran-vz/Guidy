//! Model list logic (helpers for the Models page, model store, model card and
//! model selector).

use super::languages::supports_language_code;
use guidy_core::managers::model::{ModelInfo, ModelSource};
use std::collections::{HashMap, HashSet};

/// Legacy models are the blob (Url-sourced) .bin downloads, superseded by the
/// catalog GGUFs. They stay runnable when on disk but aren't advertised.
pub fn is_legacy_model(model: &ModelInfo) -> bool {
    matches!(model.source, ModelSource::Url { .. })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelFilter {
    pub query: String,
    /// `None` = all languages.
    pub language: Option<String>,
    pub streaming: bool,
    pub translation: bool,
}

pub fn filter_models<'a>(models: &'a [ModelInfo], f: &ModelFilter) -> Vec<&'a ModelInfo> {
    let q = f.query.trim().to_lowercase();
    models
        .iter()
        .filter(|m| {
            if is_legacy_model(m) && !m.is_downloaded {
                return false;
            }
            if let Some(lang) = &f.language {
                if !supports_language_code(&m.supported_languages, lang) {
                    return false;
                }
            }
            if f.streaming && !m.supports_streaming {
                return false;
            }
            if f.translation && !m.supports_translation {
                return false;
            }
            if !q.is_empty() {
                let haystack = format!("{} {}", m.name, m.description).to_lowercase();
                if !haystack.contains(&q) {
                    return false;
                }
            }
            true
        })
        .collect()
}

/// Split into (downloaded incl. custom/downloading, available). Downloaded is
/// sorted active first, custom last (stable otherwise).
pub fn split_sections<'a>(
    models: Vec<&'a ModelInfo>,
    downloading: &HashSet<String>,
    current_model: &str,
) -> (Vec<&'a ModelInfo>, Vec<&'a ModelInfo>) {
    let (mut downloaded, available): (Vec<_>, Vec<_>) = models
        .into_iter()
        .partition(|m| m.is_custom || m.is_downloaded || downloading.contains(&m.id));
    downloaded.sort_by_key(|m| {
        if m.id == current_model {
            0
        } else if m.is_custom {
            2
        } else {
            1
        }
    });
    (downloaded, available)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelCardStatus {
    Verifying,
    Downloading,
    Switching,
    Downloadable,
    Active,
    Available,
}

pub fn model_card_status(
    model: Option<&ModelInfo>,
    model_id: &str,
    verifying: &HashSet<String>,
    downloading: &HashSet<String>,
    switching: Option<&str>,
    current_model: &str,
) -> ModelCardStatus {
    if verifying.contains(model_id) {
        return ModelCardStatus::Verifying;
    }
    if downloading.contains(model_id) {
        return ModelCardStatus::Downloading;
    }
    if switching == Some(model_id) {
        return ModelCardStatus::Switching;
    }
    // A stale persisted selection must never make a missing model look Active.
    if !model.is_some_and(|m| m.is_downloaded) {
        return ModelCardStatus::Downloadable;
    }
    if model_id == current_model {
        return ModelCardStatus::Active;
    }
    ModelCardStatus::Available
}

impl ModelCardStatus {
    /// Cards are clickable only to download or to select.
    pub fn is_clickable(self) -> bool {
        matches!(
            self,
            ModelCardStatus::Downloadable | ModelCardStatus::Available
        )
    }
    pub fn can_delete(self) -> bool {
        matches!(self, ModelCardStatus::Available | ModelCardStatus::Active)
    }
}

/// Download speed tracking (modelStore `downloadStats`): recomputed at most
/// every 0.5 s, exponentially smoothed 0.8/0.2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DownloadStats {
    pub start_ms: u64,
    pub last_update_ms: u64,
    pub total_downloaded: u64,
    /// MB/s
    pub speed: f64,
}

pub fn update_download_stats(
    stats: Option<DownloadStats>,
    now_ms: u64,
    downloaded: u64,
) -> DownloadStats {
    match stats {
        None => DownloadStats {
            start_ms: now_ms,
            last_update_ms: now_ms,
            total_downloaded: downloaded,
            speed: 0.0,
        },
        Some(current) => {
            let time_diff = now_ms.saturating_sub(current.last_update_ms) as f64 / 1000.0;
            if time_diff <= 0.5 {
                return current;
            }
            let bytes_diff = downloaded as f64 - current.total_downloaded as f64;
            let speed_now = (bytes_diff / (1024.0 * 1024.0) / time_diff).max(0.0);
            let smoothed = if current.speed > 0.0 {
                current.speed * 0.8 + speed_now * 0.2
            } else {
                speed_now
            };
            DownloadStats {
                start_ms: current.start_ms,
                last_update_ms: now_ms,
                total_downloaded: downloaded,
                speed: smoothed.max(0.0),
            }
        }
    }
}

/// "742 MB", "1.5 GB", "12 GB", "Unknown size".
pub fn format_model_size(size_mb: Option<f64>) -> String {
    let Some(mb) = size_mb.filter(|m| m.is_finite() && *m > 0.0) else {
        return "Unknown size".into();
    };
    // Intl.NumberFormat rounds half away from zero.
    let one = |v: f64| format!("{:.1}", (v * 10.0).round() / 10.0);
    let zero = |v: f64| format!("{:.0}", v.round());
    if mb >= 1024.0 {
        let gb = mb / 1024.0;
        if gb >= 10.0 {
            format!("{} GB", zero(gb))
        } else {
            format!("{} GB", one(gb))
        }
    } else if mb >= 100.0 {
        format!("{} MB", zero(mb))
    } else {
        format!("{} MB", one(mb))
    }
}

/// GGUF quantization label from a filename ("…-Q8_0.gguf" → "Q8_0").
pub fn quantization_label(filename: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?i)[._-](IQ\d+_\w+|Q\d+(?:_\w+)?|F16|BF16|F32)\.gguf$").unwrap()
    });
    re.captures(filename).map(|c| c[1].to_uppercase())
}

/// Footer model selector status (ModelSelector `ModelStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectorStatus {
    Ready,
    Loading,
    Downloading,
    Verifying,
    Error,
    Unloaded,
    None,
}

/// Apply a `model-state-changed` event type to the selector status.
pub fn selector_status_after_event(current: SelectorStatus, event_type: &str) -> SelectorStatus {
    match event_type {
        "loading_started" => SelectorStatus::Loading,
        "loading_completed" => SelectorStatus::Ready,
        "loading_failed" => SelectorStatus::Error,
        "unloaded" => SelectorStatus::Unloaded,
        _ => current,
    }
}

/// Initial selector status from `get_transcription_model_status` vs the
/// current model.
pub fn initial_selector_status(loaded_model: Option<&str>, current_model: &str) -> SelectorStatus {
    if current_model.is_empty() {
        return SelectorStatus::None;
    }
    match loaded_model {
        Some(id) if id == current_model => SelectorStatus::Ready,
        _ => SelectorStatus::Unloaded,
    }
}

/// Progress percentages of in-flight downloads, in a stable order.
pub fn active_downloads(progress: &HashMap<String, f64>) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = progress.iter().map(|(k, p)| (k.clone(), *p)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Onboarding picker groups (Onboarding.tsx): on-disk models; downloadable
/// (non-legacy) split into 2 featured recommended, other recommended, rest.
#[derive(Debug, Default)]
pub struct PickerGroups<'a> {
    pub existing: Vec<&'a ModelInfo>,
    pub downloadable: Vec<&'a ModelInfo>,
    pub top_picks: Vec<&'a ModelInfo>,
    pub other_recommended: Vec<&'a ModelInfo>,
    pub rest: Vec<&'a ModelInfo>,
}

pub fn picker_groups(models: &[ModelInfo]) -> PickerGroups<'_> {
    let downloadable: Vec<&ModelInfo> = models
        .iter()
        .filter(|m| !m.is_downloaded && !is_legacy_model(m))
        .collect();
    let recommended: Vec<&ModelInfo> = downloadable
        .iter()
        .copied()
        .filter(|m| m.is_recommended)
        .collect();
    PickerGroups {
        existing: models.iter().filter(|m| m.is_downloaded).collect(),
        top_picks: recommended.iter().take(2).copied().collect(),
        other_recommended: recommended.iter().skip(2).copied().collect(),
        rest: downloadable
            .iter()
            .copied()
            .filter(|m| !m.is_recommended)
            .collect(),
        downloadable,
    }
}

/// The picked model finished downloading/verifying and can be selected.
pub fn picked_model_ready(
    models: &[ModelInfo],
    picked: &str,
    downloading: &HashSet<String>,
    verifying: &HashSet<String>,
) -> bool {
    models.iter().any(|m| m.id == picked && m.is_downloaded)
        && !downloading.contains(picked)
        && !verifying.contains(picked)
}

#[cfg(test)]
pub(crate) fn test_model(id: &str) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        name: id.to_uppercase(),
        description: format!("{id} description"),
        filename: format!("{id}-Q8_0.gguf"),
        source: ModelSource::HuggingFace {
            repo_id: format!("org/{id}"),
            revision: "main".into(),
        },
        size_mb: 500,
        is_downloaded: false,
        is_downloading: false,
        partial_size: 0,
        accuracy_score: 0.5,
        speed_score: 0.5,
        supports_translation: false,
        is_recommended: false,
        supported_languages: vec!["en".into()],
        supports_language_selection: true,
        is_custom: false,
        supports_streaming: false,
        supports_language_detection: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn filter_hides_undownloaded_legacy_and_applies_toggles() {
        let mut legacy = test_model("legacy");
        legacy.source = ModelSource::Url {
            url: "x".into(),
            sha256: None,
        };
        let mut streaming = test_model("live");
        streaming.supports_streaming = true;
        streaming.supported_languages = vec!["de".into()];
        let models = vec![legacy.clone(), streaming, test_model("plain")];

        let all = filter_models(&models, &ModelFilter::default());
        assert_eq!(
            all.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["live", "plain"]
        );

        let f = ModelFilter {
            streaming: true,
            ..Default::default()
        };
        assert_eq!(filter_models(&models, &f).len(), 1);
        let f = ModelFilter {
            language: Some("de".into()),
            ..Default::default()
        };
        assert_eq!(filter_models(&models, &f)[0].id, "live");
        let f = ModelFilter {
            query: "PLAIN desc".into(),
            ..Default::default()
        };
        assert_eq!(filter_models(&models, &f)[0].id, "plain");

        legacy.is_downloaded = true;
        assert_eq!(filter_models(&[legacy], &ModelFilter::default()).len(), 1);
    }

    #[test]
    fn sections_sort_active_first_custom_last() {
        let mut a = test_model("a");
        a.is_downloaded = true;
        let mut custom = test_model("custom");
        custom.is_custom = true;
        let mut active = test_model("active");
        active.is_downloaded = true;
        let dl = test_model("dl");
        let other = test_model("other");
        let models = [
            custom.clone(),
            a.clone(),
            active.clone(),
            dl.clone(),
            other.clone(),
        ];
        let (downloaded, available) =
            split_sections(models.iter().collect(), &set(&["dl"]), "active");
        assert_eq!(
            downloaded.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["active", "a", "dl", "custom"]
        );
        assert_eq!(
            available.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["other"]
        );
    }

    #[test]
    fn card_status_precedence() {
        let mut m = test_model("m");
        m.is_downloaded = true;
        let empty = HashSet::new();
        assert_eq!(
            model_card_status(Some(&m), "m", &set(&["m"]), &set(&["m"]), Some("m"), "m"),
            ModelCardStatus::Verifying
        );
        assert_eq!(
            model_card_status(Some(&m), "m", &empty, &set(&["m"]), Some("m"), "m"),
            ModelCardStatus::Downloading
        );
        assert_eq!(
            model_card_status(Some(&m), "m", &empty, &empty, Some("m"), "m"),
            ModelCardStatus::Switching
        );
        assert_eq!(
            model_card_status(Some(&m), "m", &empty, &empty, None, "m"),
            ModelCardStatus::Active
        );
        assert_eq!(
            model_card_status(Some(&m), "m", &empty, &empty, None, "x"),
            ModelCardStatus::Available
        );
        m.is_downloaded = false;
        assert_eq!(
            model_card_status(Some(&m), "m", &empty, &empty, None, "m"),
            ModelCardStatus::Downloadable,
            "stale selection never looks active"
        );
        assert!(ModelCardStatus::Downloadable.is_clickable());
        assert!(!ModelCardStatus::Active.is_clickable());
        assert!(ModelCardStatus::Active.can_delete());
    }

    #[test]
    fn download_speed_is_smoothed_and_throttled() {
        let s0 = update_download_stats(None, 1_000, 0);
        assert_eq!(s0.speed, 0.0);
        // Within 0.5 s: unchanged
        assert_eq!(update_download_stats(Some(s0), 1_400, 5 << 20), s0);
        // 1 s later, 10 MB → 10 MB/s (first sample unsmoothed)
        let s1 = update_download_stats(Some(s0), 2_000, 10 << 20);
        assert!((s1.speed - 10.0).abs() < 1e-9);
        // next 1 s, 20 MB → 0.8*10 + 0.2*20 = 12
        let s2 = update_download_stats(Some(s1), 3_000, 30 << 20);
        assert!((s2.speed - 12.0).abs() < 1e-9);
        assert_eq!(s2.start_ms, 1_000);
    }

    #[test]
    fn sizes_and_quantization() {
        assert_eq!(format_model_size(Some(42.25)), "42.3 MB");
        assert_eq!(format_model_size(Some(742.0)), "742 MB");
        assert_eq!(format_model_size(Some(1536.0)), "1.5 GB");
        assert_eq!(format_model_size(Some(12288.0)), "12 GB");
        assert_eq!(format_model_size(None), "Unknown size");
        assert_eq!(format_model_size(Some(0.0)), "Unknown size");
        assert_eq!(
            quantization_label("parakeet-0.6b-Q8_0.gguf").as_deref(),
            Some("Q8_0")
        );
        assert_eq!(
            quantization_label("x-Q5_K_M.gguf").as_deref(),
            Some("Q5_K_M")
        );
        assert_eq!(quantization_label("ggml-small.bin"), None);
        assert_eq!(
            quantization_label("m.IQ4_XS.gguf").as_deref(),
            Some("IQ4_XS")
        );
        assert_eq!(quantization_label("m-f16.gguf").as_deref(), Some("F16"));
    }

    #[test]
    fn selector_status_transitions() {
        use SelectorStatus::*;
        assert_eq!(initial_selector_status(Some("a"), "a"), Ready);
        assert_eq!(initial_selector_status(Option::None, "a"), Unloaded);
        assert_eq!(
            initial_selector_status(Option::None, ""),
            SelectorStatus::None
        );
        assert_eq!(
            selector_status_after_event(Ready, "loading_started"),
            Loading
        );
        assert_eq!(
            selector_status_after_event(Loading, "loading_failed"),
            Error
        );
        assert_eq!(
            selector_status_after_event(Loading, "selection_changed"),
            Loading
        );
    }

    #[test]
    fn picker_groups_feature_two_recommended() {
        let mut ms: Vec<ModelInfo> = ["r1", "r2", "r3", "x", "have"]
            .iter()
            .map(|i| test_model(i))
            .collect();
        for m in ms.iter_mut().take(3) {
            m.is_recommended = true;
        }
        ms[4].is_downloaded = true;
        let mut legacy = test_model("legacy");
        legacy.source = ModelSource::Url {
            url: "u".into(),
            sha256: None,
        };
        ms.push(legacy);
        let g = picker_groups(&ms);
        let ids = |v: &[&ModelInfo]| v.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&g.existing), ["have"]);
        assert_eq!(ids(&g.top_picks), ["r1", "r2"]);
        assert_eq!(ids(&g.other_recommended), ["r3"]);
        assert_eq!(ids(&g.rest), ["x"]);
        assert_eq!(g.downloadable.len(), 4, "legacy never offered");
    }

    #[test]
    fn picked_model_ready_waits_for_download_and_verify() {
        let mut m = test_model("m");
        let empty = HashSet::new();
        assert!(!picked_model_ready(&[m.clone()], "m", &empty, &empty));
        m.is_downloaded = true;
        assert!(!picked_model_ready(&[m.clone()], "m", &set(&["m"]), &empty));
        assert!(!picked_model_ready(&[m.clone()], "m", &empty, &set(&["m"])));
        assert!(picked_model_ready(&[m], "m", &empty, &empty));
    }
}
