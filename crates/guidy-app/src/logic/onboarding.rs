//! First-run setup: what the user picked on the Welcome and "Choose your
//! setup" steps, and what that means in models, downloads and settings.
//! Pure (no GPUI, no I/O).
//!
//! The plan asks [`requirements`](super::requirements) with the settings
//! the choices would write (the speech model selected, meetings on with the
//! chosen summaries provider), so the rows, sizes and "nothing extra" notes
//! are exactly what the app will use afterwards.

use super::languages::supports_language_code;
use super::models::{filter_models, ModelFilter};
use super::requirements::{
    self, recommended_stt, Choice, ChoiceKind, Feature, Inputs, Requirement, Role,
};
use guidy_core::downloads::DownloadKind;
use guidy_core::managers::model::ModelInfo;
use guidy_core::managers::transcription::meeting::DEFAULT_MEETING_MODEL_ID;
use guidy_core::meetings::processing::speaker_model::{
    segmentation_download_id, separation_download_id,
};
use guidy_core::meetings::settings::BUILTIN_LLM_PROVIDER_ID;
use guidy_core::meetings::types::SpeakerModelId;
use guidy_core::settings::{AppSettings, APPLE_INTELLIGENCE_PROVIDER_ID};
use serde_json::{json, Value};

/// Where meeting summaries are written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Summaries {
    /// Apple Intelligence (no download).
    AppleIntelligence,
    /// The built-in local LLM.
    OnThisMac,
    /// A cloud provider (provider id) with the user's API key.
    Cloud(String),
}

impl Summaries {
    /// The `meetings.llm_provider_id` it stands for.
    pub fn provider_id(&self) -> &str {
        match self {
            Self::AppleIntelligence => APPLE_INTELLIGENCE_PROVIDER_ID,
            Self::OnThisMac => BUILTIN_LLM_PROVIDER_ID,
            Self::Cloud(id) => id,
        }
    }

    fn of_choice(c: &Choice) -> Self {
        match c.kind {
            ChoiceKind::Local => Self::OnThisMac,
            ChoiceKind::OnDevice => Self::AppleIntelligence,
            ChoiceKind::Cloud => Self::Cloud(c.id.clone()),
        }
    }
}

/// What the user picked during onboarding. Lives on the app state, so it
/// survives Back, and the window being closed and reopened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    pub dictation: bool,
    pub meetings: bool,
    /// A speech model picked with "Choose Another…" (`None`: the
    /// recommended one).
    pub speech_model: Option<String>,
    /// `None`: the recommended place.
    pub summaries: Option<Summaries>,
    /// The optional speaker-turn models.
    pub better_turns: bool,
}

impl Default for Choices {
    fn default() -> Self {
        Self {
            dictation: true,
            meetings: false,
            speech_model: None,
            summaries: None,
            better_turns: false,
        }
    }
}

impl Choices {
    /// At least one feature is chosen.
    pub fn any(&self) -> bool {
        self.dictation || self.meetings
    }
}

/// The models the setup step shows, and what Continue downloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The speech model (model id), for dictation and/or meetings.
    pub speech_model: Option<String>,
    /// Dictation's speech model (when dictation is chosen).
    pub speech: Option<Choice>,
    /// The meeting transcription model (when meetings are chosen).
    pub transcription: Option<Choice>,
    /// Meetings transcribe with the dictation model: nothing extra.
    pub transcription_is_speech: bool,
    pub speaker: Option<Choice>,
    /// Where summaries are written (resolved: the user's pick or the
    /// recommended one).
    pub summaries: Summaries,
    pub apple: Option<Choice>,
    /// The local LLM summaries would use.
    pub local_llm: Option<Choice>,
    /// The optional speaker-turn models (segmentation + separation).
    pub turns: Vec<Choice>,
    /// Local models to queue, in order (dictation's model first).
    pub downloads: Vec<Choice>,
}

impl Plan {
    pub fn download_ids(&self) -> Vec<String> {
        self.downloads.iter().map(|c| c.id.clone()).collect()
    }

    pub fn download_bytes(&self) -> u64 {
        self.downloads.iter().map(|c| c.download_bytes).sum()
    }

    pub fn turns_bytes(&self) -> u64 {
        self.turns.iter().map(|c| c.download_bytes).sum()
    }
}

/// The language the plan recommends for: the dictation language when one
/// is set, else the UI language.
pub fn plan_language(settings: &AppSettings, ui_language: &str) -> String {
    let lang = settings.selected_language.trim();
    if lang.is_empty() || lang == "auto" {
        ui_language.to_string()
    } else {
        lang.to_string()
    }
}

/// The speech model to suggest: the recommended one for the language; with
/// meeting notes, Parakeet v3 when it speaks the language and nothing
/// usable is downloaded, so one model covers both features.
pub fn default_speech_model(inputs: &Inputs, meetings: bool) -> Option<String> {
    let recommended = recommended_stt(inputs);
    if meetings && !recommended.is_some_and(|m| m.is_downloaded) {
        let lang = inputs.language.split(['-', '_']).next().unwrap_or("en");
        if let Some(m) = inputs
            .stt_models
            .iter()
            .find(|m| m.id == DEFAULT_MEETING_MODEL_ID)
            .filter(|m| supports_language_code(&m.supported_languages, lang))
        {
            return Some(m.id.clone());
        }
    }
    recommended.map(|m| m.id.clone())
}

/// The summaries place to suggest when the user hasn't picked one
/// (requirements' recommendation; the local model when there is none).
pub fn default_summaries(inputs: &Inputs) -> Summaries {
    requirements::plan(inputs, Feature::MeetingNotes)
        .requirement(Role::Summaries)
        .and_then(Requirement::recommended)
        .map(Summaries::of_choice)
        .unwrap_or(Summaries::OnThisMac)
}

/// The settings `choices` would write, applied to `base` (for planning).
fn planned_settings(base: &AppSettings, choices: &Choices, speech: Option<&str>) -> AppSettings {
    let mut s = base.clone();
    if let Some(id) = speech {
        if choices.dictation {
            s.selected_model = id.to_string();
        } else {
            s.meetings.meeting_model = Some(id.to_string());
        }
    }
    s
}

pub fn plan(inputs: &Inputs, choices: &Choices) -> Plan {
    let speech_model = choices
        .speech_model
        .clone()
        .filter(|id| inputs.stt_models.iter().any(|m| &m.id == id))
        .or_else(|| default_speech_model(inputs, choices.meetings));
    let summaries = choices
        .summaries
        .clone()
        .unwrap_or_else(|| default_summaries(inputs));
    let mut settings = planned_settings(inputs.settings, choices, speech_model.as_deref());
    settings.meetings.llm_provider_id = summaries.provider_id().to_string();
    let planned = Inputs {
        settings: &settings,
        ..*inputs
    };

    let mut downloads: Vec<Choice> = Vec::new();
    let mut queue = |c: &Choice| {
        if c.kind == ChoiceKind::Local && !c.ready && !downloads.iter().any(|d| d.id == c.id) {
            downloads.push(c.clone());
        }
    };

    let speech = choices.dictation.then(|| {
        requirements::plan(&planned, Feature::Dictation)
            .requirement(Role::SpeechModel)
            .and_then(Requirement::in_use)
            .cloned()
    });
    let speech = speech.flatten();
    if let Some(c) = &speech {
        queue(c);
    }

    let mut out = Plan {
        speech_model,
        speech,
        transcription: None,
        transcription_is_speech: false,
        speaker: None,
        summaries,
        apple: None,
        local_llm: None,
        turns: Vec::new(),
        downloads: Vec::new(),
    };
    if choices.meetings {
        let m = requirements::plan(&planned, Feature::MeetingNotes);
        let pick = |role: Role| m.requirement(role).and_then(Requirement::in_use).cloned();
        out.transcription = pick(Role::MeetingTranscription);
        out.transcription_is_speech = out
            .transcription
            .as_ref()
            .zip(out.speech.as_ref())
            .is_some_and(|(t, s)| t.id == s.id);
        out.speaker = pick(Role::SpeakerRecognition);
        if let Some(r) = m.requirement(Role::Summaries) {
            out.apple = r
                .choices
                .iter()
                .find(|c| c.kind == ChoiceKind::OnDevice)
                .cloned();
            // What the built-in provider would run: the in-use local model
            // when it is the pick, else the default one.
            out.local_llm = r
                .choices
                .iter()
                .find(|c| c.kind == ChoiceKind::Local && c.in_use)
                .or_else(|| {
                    let default = inputs
                        .other_models
                        .iter()
                        .find(|o| o.kind == DownloadKind::Llm && o.is_default)?;
                    r.choices.iter().find(|c| c.id == default.id)
                })
                .cloned();
        }
        for c in [&out.transcription, &out.speaker].into_iter().flatten() {
            queue(c);
        }
        if out.summaries == Summaries::OnThisMac {
            if let Some(c) = &out.local_llm {
                queue(c);
            }
        }
        out.turns = [
            segmentation_download_id().to_string(),
            separation_download_id(SpeakerModelId::TitanetLarge),
        ]
        .iter()
        .filter_map(|id| inputs.other_models.iter().find(|o| &o.id == id))
        .map(|o| {
            let mut c = Choice {
                id: o.id.clone(),
                name: o.name.clone(),
                kind: ChoiceKind::Local,
                size_bytes: o.size_bytes,
                download_bytes: 0,
                ready: o.installed,
                available: true,
                recommended: false,
                in_use: choices.better_turns,
            };
            if !o.installed {
                c.download_bytes = o.size_bytes.saturating_sub(o.partial_bytes);
            }
            c
        })
        .collect();
        if choices.better_turns {
            for c in &out.turns {
                queue(c);
            }
        }
    }
    out.downloads = downloads;
    out
}

/// The writes that finish the setup, in order (`update_setting` keys and
/// values). Selecting the dictation model goes through the model commands
/// instead (it loads it).
pub fn setting_writes(choices: &Choices, plan: &Plan) -> Vec<(&'static str, Value)> {
    if !choices.meetings {
        return Vec::new();
    }
    let mut patch = json!({
        "enabled": true,
        "llm_provider_id": plan.summaries.provider_id(),
        "speaker_turn_models_consent": choices.better_turns,
    });
    if !choices.dictation {
        if let Some(id) = &plan.speech_model {
            patch["meeting_model"] = json!(id);
        }
    }
    vec![("meetings", patch)]
}

/// The speech models the "Choose Another…" list shows: everything that can
/// be used or downloaded (no retired formats unless on disk), filtered by
/// language (`None`: all) and a search. Downloaded first, then recommended,
/// then the rest, each by accuracy + speed.
pub fn picker_models<'a>(
    models: &'a [ModelInfo],
    language: Option<&str>,
    query: &str,
) -> Vec<&'a ModelInfo> {
    let filter = ModelFilter {
        query: query.to_string(),
        language: language.map(str::to_string),
        ..Default::default()
    };
    let mut list = filter_models(models, &filter);
    let score = |m: &ModelInfo| m.accuracy_score + m.speed_score;
    list.sort_by(|a, b| {
        b.is_downloaded
            .cmp(&a.is_downloaded)
            .then(b.is_recommended.cmp(&a.is_recommended))
            .then(score(b).total_cmp(&score(a)))
            .then_with(|| a.name.cmp(&b.name))
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::models::test_model;
    use guidy_core::downloads::OtherModel;

    fn stt(id: &str, langs: &[&str], recommended: bool, mb: u64, score: f32) -> ModelInfo {
        let mut m = test_model(id);
        m.supported_languages = langs.iter().map(|s| s.to_string()).collect();
        m.is_recommended = recommended;
        m.size_mb = mb;
        m.accuracy_score = score;
        m.speed_score = score;
        m
    }

    fn other(id: &str, kind: DownloadKind, size: u64, default: bool) -> OtherModel {
        OtherModel {
            id: id.into(),
            kind,
            name: id.into(),
            description: String::new(),
            size_bytes: size,
            installed: false,
            partial_bytes: 0,
            is_default: default,
            recommended_min_ram_gb: None,
            fits_this_mac: true,
        }
    }

    struct World {
        stt: Vec<ModelInfo>,
        other: Vec<OtherModel>,
        settings: AppSettings,
        keys: Vec<String>,
        ai: bool,
        lang: &'static str,
    }

    impl World {
        fn new() -> Self {
            let mut settings = guidy_core::settings::get_default_settings();
            settings.selected_model = String::new();
            Self {
                stt: vec![
                    stt("english", &["en"], true, 600, 0.9),
                    stt(
                        DEFAULT_MEETING_MODEL_ID,
                        &["en", "de", "fr"],
                        true,
                        740,
                        0.8,
                    ),
                    stt("whisper", &["en", "de", "ja"], false, 465, 0.6),
                ],
                other: vec![
                    other("llm:qwen", DownloadKind::Llm, 2_500_000_000, true),
                    other("speaker:resnet34", DownloadKind::Speaker, 26_530_550, true),
                    other(
                        "separation:titanet_large",
                        DownloadKind::Separation,
                        101_405_493,
                        false,
                    ),
                    other(
                        "segmentation:pyannote",
                        DownloadKind::Segmentation,
                        5_992_913,
                        false,
                    ),
                ],
                settings,
                keys: Vec::new(),
                ai: false,
                lang: "en",
            }
        }

        fn inputs(&self) -> Inputs<'_> {
            Inputs {
                stt_models: &self.stt,
                other_models: &self.other,
                settings: &self.settings,
                providers_with_keys: &self.keys,
                apple_intelligence: self.ai,
                language: self.lang,
                ram_gb: Some(16),
            }
        }
    }

    fn stt_id(id: &str) -> String {
        guidy_core::downloads::stt_id(id)
    }

    #[test]
    fn dictation_only_needs_one_speech_model() {
        let w = World::new();
        let p = plan(&w.inputs(), &Choices::default());
        assert_eq!(p.speech_model.as_deref(), Some("english"));
        assert_eq!(p.download_ids(), vec![stt_id("english")]);
        assert_eq!(p.download_bytes(), 600 * 1024 * 1024);
        assert!(p.transcription.is_none() && p.speaker.is_none());
    }

    #[test]
    fn meetings_pick_one_model_for_both_and_add_speakers_and_summaries() {
        let w = World::new();
        let choices = Choices {
            meetings: true,
            ..Default::default()
        };
        let p = plan(&w.inputs(), &choices);
        // Parakeet v3 covers dictation and meetings: nothing extra.
        assert_eq!(p.speech_model.as_deref(), Some(DEFAULT_MEETING_MODEL_ID));
        assert!(p.transcription_is_speech);
        assert_eq!(p.summaries, Summaries::OnThisMac);
        assert_eq!(
            p.download_ids(),
            vec![
                stt_id(DEFAULT_MEETING_MODEL_ID),
                "speaker:resnet34".to_string(),
                "llm:qwen".to_string()
            ]
        );
    }

    #[test]
    fn welcome_choices_drive_the_plan() {
        let w = World::new();
        // Meetings only: the meeting model is written explicitly.
        let choices = Choices {
            dictation: false,
            meetings: true,
            summaries: Some(Summaries::Cloud("openai".into())),
            ..Default::default()
        };
        let p = plan(&w.inputs(), &choices);
        assert!(p.speech.is_none());
        assert_eq!(p.summaries, Summaries::Cloud("openai".into()));
        assert_eq!(
            p.download_ids(),
            vec![
                stt_id(DEFAULT_MEETING_MODEL_ID),
                "speaker:resnet34".to_string()
            ],
            "cloud summaries download no LLM"
        );
        let writes = setting_writes(&choices, &p);
        assert_eq!(writes[0].1["enabled"], true);
        assert_eq!(writes[0].1["llm_provider_id"], "openai");
        assert_eq!(writes[0].1["meeting_model"], DEFAULT_MEETING_MODEL_ID);
        // Nothing chosen: nothing to download or write.
        let none = Choices {
            dictation: false,
            ..Default::default()
        };
        let p = plan(&w.inputs(), &none);
        assert!(p.downloads.is_empty());
        assert!(setting_writes(&none, &p).is_empty());
    }

    #[test]
    fn another_speech_model_and_better_turns_change_the_downloads() {
        let w = World::new();
        let choices = Choices {
            meetings: true,
            speech_model: Some("whisper".into()),
            summaries: Some(Summaries::AppleIntelligence),
            better_turns: true,
            ..Default::default()
        };
        let p = plan(&w.inputs(), &choices);
        assert!(p.transcription_is_speech);
        assert_eq!(
            p.download_ids(),
            vec![
                stt_id("whisper"),
                "speaker:resnet34".to_string(),
                "segmentation:pyannote".to_string(),
                "separation:titanet_large".to_string(),
            ]
        );
        assert_eq!(p.turns_bytes(), 5_992_913 + 101_405_493);
        assert_eq!(
            setting_writes(&choices, &p)[0].1["speaker_turn_models_consent"],
            true
        );
    }

    #[test]
    fn a_language_parakeet_lacks_keeps_the_language_model() {
        let mut w = World::new();
        w.lang = "ja";
        let choices = Choices {
            meetings: true,
            ..Default::default()
        };
        let p = plan(&w.inputs(), &choices);
        assert_eq!(p.speech_model.as_deref(), Some("whisper"));
    }

    #[test]
    fn downloaded_models_need_nothing() {
        let mut w = World::new();
        w.stt[0].is_downloaded = true;
        w.ai = true;
        let p = plan(&w.inputs(), &Choices::default());
        assert!(p.downloads.is_empty());
        assert!(p.speech.as_ref().is_some_and(|c| c.ready));
    }

    #[test]
    fn picker_filters_by_language_and_lists_downloaded_first() {
        let mut w = World::new();
        w.stt[2].is_downloaded = true;
        let all: Vec<_> = picker_models(&w.stt, None, "")
            .iter()
            .map(|m| m.id.clone())
            .collect();
        assert_eq!(all[0], "whisper");
        let ja: Vec<_> = picker_models(&w.stt, Some("ja"), "")
            .iter()
            .map(|m| m.id.clone())
            .collect();
        assert_eq!(ja, vec!["whisper"]);
        assert!(picker_models(&w.stt, None, "ENGL").len() == 1);
    }
}
