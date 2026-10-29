//! What each feature needs, what the user already has, and what to
//! download. Pure (no GPUI, no I/O): build [`Inputs`] from the app state and
//! ask.
//!
//! Used by the Models page (badges "Required by Dictation · Meetings",
//! recommended setups), onboarding ("Choose your setup") and Meetings setup.
//!
//! # Model
//!
//! - A [`Feature`] (Dictation, Meeting notes, Post-processing) has
//!   [`Requirement`]s, one per [`Role`] (speech model, meeting
//!   transcription, speaker recognition, summaries…).
//! - A requirement offers [`Choice`]s: a local model (by download-tracker id,
//!   `stt:…`, `llm:…`, `speaker:resnet34`, `separation:…`), Apple
//!   Intelligence ([`ChoiceKind::OnDevice`], no download) or a cloud
//!   provider ([`ChoiceKind::Cloud`], needs an API key). Each choice says
//!   whether it is `ready` (on disk / available / key saved), `in_use` (the
//!   current settings point to it) and `recommended`.
//! - [`plan`] gives a [`FeaturePlan`]; [`FeaturePlan::ready`] says whether the
//!   feature works now and [`FeaturePlan::downloads`] what the recommended
//!   choices still need.
//! - [`setup`] gives the three ready-made [`Setup`]s ("Dictation", "Meetings
//!   on this Mac", "Meetings with cloud AI") as a list of download ids and
//!   their total size: enqueue them with `AppState::request_downloads`.
//! - [`required_by`] lists the enabled features that use a model now.
//!
//! # Recommendations
//!
//! - Speech: a downloaded model that speaks the system language if there is
//!   one, else the catalog's recommended models that speak it (best sum of
//!   accuracy and speed, then smaller), else any model that speaks it, else
//!   the first recommended one.
//! - Meeting transcription: what meetings would use now
//!   (`resolve_meeting_model_id`: configured → dictation model → any
//!   Parakeet v3), else the default meeting model (Parakeet TDT 0.6B v3).
//! - Summaries / post-processing: the in-use choice when it is ready; else
//!   the default local LLM when this Mac has the memory for it (≥ 8 GB or
//!   unknown); else Apple Intelligence when available; else a cloud
//!   provider with a key.

use super::languages::supports_language_code;
use super::models::is_legacy_model;
use guidy_core::downloads::{DownloadKind, OtherModel};
use guidy_core::managers::model::ModelInfo;
use guidy_core::managers::transcription::meeting::{
    resolve_meeting_model_id, DEFAULT_MEETING_MODEL_ID,
};
use guidy_core::meetings::settings::BUILTIN_LLM_PROVIDER_ID;
use guidy_core::settings::{AppSettings, APPLE_INTELLIGENCE_PROVIDER_ID};

/// A user-facing capability that needs models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    Dictation,
    MeetingNotes,
    PostProcessing,
}

impl Feature {
    pub const ALL: [Feature; 3] = [Self::Dictation, Self::MeetingNotes, Self::PostProcessing];

    /// i18n key of its short name ("Dictation", "Meetings", …).
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Dictation => "models.features.dictation",
            Self::MeetingNotes => "models.features.meetings",
            Self::PostProcessing => "models.features.postProcessing",
        }
    }

    /// Turned on in the settings (dictation always is).
    pub fn enabled(self, settings: &AppSettings) -> bool {
        match self {
            Self::Dictation => true,
            Self::MeetingNotes => settings.meetings.enabled,
            Self::PostProcessing => settings.post_process_enabled,
        }
    }
}

/// What a requirement is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Dictation's speech-to-text model.
    SpeechModel,
    /// The model meetings are transcribed with.
    MeetingTranscription,
    /// The standard speaker model (who spoke; recognizes known people).
    SpeakerRecognition,
    /// A larger speaker model chosen for telling voices apart (optional).
    SpeakerSeparation,
    /// The LLM that writes meeting summaries.
    Summaries,
    /// The LLM that cleans up dictation.
    PostProcessing,
}

impl Role {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::SpeechModel => "models.roles.speechModel",
            Self::MeetingTranscription => "models.roles.meetingTranscription",
            Self::SpeakerRecognition => "models.roles.speakerRecognition",
            Self::SpeakerSeparation => "models.roles.speakerSeparation",
            Self::Summaries => "models.roles.summaries",
            Self::PostProcessing => "models.roles.postProcessing",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceKind {
    /// A model file; `Choice::id` is its download-tracker id.
    Local,
    /// Built into the OS, nothing to download (Apple Intelligence).
    OnDevice,
    /// A cloud provider (`Choice::id` = provider id); needs an API key.
    Cloud,
}

/// One way to meet a requirement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// Download-tracker id (local) or provider id (on-device, cloud).
    pub id: String,
    pub name: String,
    pub kind: ChoiceKind,
    /// Full size on disk (0 for on-device and cloud).
    pub size_bytes: u64,
    /// Still to download (0 when on disk; a partial counts as done bytes).
    pub download_bytes: u64,
    /// Usable now: on disk, available on this Mac, or API key saved.
    pub ready: bool,
    /// Can be used on this Mac at all (Apple Intelligence support, enough
    /// memory for a local LLM).
    pub available: bool,
    pub recommended: bool,
    /// The current settings use it.
    pub in_use: bool,
}

impl Choice {
    fn local(id: String, name: String, size: u64, partial: u64, ready: bool) -> Self {
        Self {
            id,
            name,
            kind: ChoiceKind::Local,
            size_bytes: size,
            download_bytes: if ready {
                0
            } else {
                size.saturating_sub(partial)
            },
            ready,
            available: true,
            recommended: false,
            in_use: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub feature: Feature,
    pub role: Role,
    /// False: an optional improvement (the feature works without it).
    pub required: bool,
    pub choices: Vec<Choice>,
}

impl Requirement {
    pub fn in_use(&self) -> Option<&Choice> {
        self.choices.iter().find(|c| c.in_use)
    }

    pub fn recommended(&self) -> Option<&Choice> {
        self.choices.iter().find(|c| c.recommended)
    }

    /// The in-use choice is ready (or the requirement is optional).
    pub fn satisfied(&self) -> bool {
        !self.required || self.in_use().is_some_and(|c| c.ready)
    }

    /// What the user should get: the in-use choice when ready, else the
    /// recommended one.
    pub fn suggestion(&self) -> Option<&Choice> {
        self.in_use()
            .filter(|c| c.ready)
            .or_else(|| self.recommended())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeaturePlan {
    pub feature: Feature,
    pub requirements: Vec<Requirement>,
}

impl FeaturePlan {
    /// Every required requirement is met with the current settings.
    pub fn ready(&self) -> bool {
        self.requirements.iter().all(Requirement::satisfied)
    }

    /// Local models the suggested choices of unmet required requirements
    /// still need (in requirement order, no duplicates).
    pub fn downloads(&self) -> Vec<&Choice> {
        let mut out: Vec<&Choice> = Vec::new();
        for r in self.requirements.iter().filter(|r| r.required) {
            if let Some(c) = r.suggestion() {
                if c.kind == ChoiceKind::Local && !c.ready && !out.iter().any(|o| o.id == c.id) {
                    out.push(c);
                }
            }
        }
        out
    }

    pub fn download_bytes(&self) -> u64 {
        self.downloads().iter().map(|c| c.download_bytes).sum()
    }

    pub fn requirement(&self, role: Role) -> Option<&Requirement> {
        self.requirements.iter().find(|r| r.role == role)
    }
}

/// Everything the plan looks at. Borrowed from the app state.
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub stt_models: &'a [ModelInfo],
    /// LLMs and speaker models (`Backend::other_models`).
    pub other_models: &'a [OtherModel],
    pub settings: &'a AppSettings,
    /// Provider ids with an API key saved.
    pub providers_with_keys: &'a [String],
    pub apple_intelligence: bool,
    /// System (or app) language, e.g. "de" or "pt-BR".
    pub language: &'a str,
    pub ram_gb: Option<u32>,
}

impl Inputs<'_> {
    fn has_key(&self, provider_id: &str) -> bool {
        self.providers_with_keys.iter().any(|p| p == provider_id)
    }

    fn base_language(&self) -> &str {
        let l = self.language.trim();
        let base = l.split(['-', '_']).next().unwrap_or(l);
        if base.is_empty() {
            "en"
        } else {
            base
        }
    }
}

const MIB: u64 = 1024 * 1024;

fn stt_bytes(m: &ModelInfo) -> u64 {
    m.size_mb * MIB
}

fn stt_choice(m: &ModelInfo) -> Choice {
    let mut c = Choice::local(
        guidy_core::downloads::stt_id(&m.id),
        m.name.clone(),
        stt_bytes(m),
        m.partial_size,
        m.is_downloaded,
    );
    c.download_bytes = if m.is_downloaded {
        0
    } else {
        stt_bytes(m).saturating_sub(m.partial_size)
    };
    c
}

fn other_choice(m: &OtherModel) -> Choice {
    let mut c = Choice::local(
        m.id.clone(),
        m.name.clone(),
        m.size_bytes,
        m.partial_bytes,
        m.installed,
    );
    c.available = m.fits_this_mac;
    c
}

/// The speech model to suggest for dictation in `inputs.language`.
pub fn recommended_stt<'a>(inputs: &Inputs<'a>) -> Option<&'a ModelInfo> {
    let lang = inputs.base_language();
    let usable: Vec<&ModelInfo> = inputs
        .stt_models
        .iter()
        .filter(|m| !is_legacy_model(m) && !m.is_custom)
        .collect();
    let speaks = |m: &&ModelInfo| supports_language_code(&m.supported_languages, lang);
    let score = |m: &ModelInfo| m.accuracy_score + m.speed_score;
    let best = |v: Vec<&'a ModelInfo>| {
        v.into_iter().max_by(|a, b| {
            score(a)
                .total_cmp(&score(b))
                .then(b.size_mb.cmp(&a.size_mb))
        })
    };
    if let Some(m) = best(
        inputs
            .stt_models
            .iter()
            .filter(|m| m.is_downloaded && speaks(m))
            .collect(),
    ) {
        return Some(m);
    }
    best(
        usable
            .iter()
            .copied()
            .filter(|m| m.is_recommended && speaks(m))
            .collect(),
    )
    .or_else(|| best(usable.iter().copied().filter(|m| speaks(m)).collect()))
    .or_else(|| usable.iter().copied().find(|m| m.is_recommended))
}

fn dictation(inputs: &Inputs) -> FeaturePlan {
    let selected = inputs.settings.selected_model.as_str();
    let recommended = recommended_stt(inputs).map(|m| m.id.clone());
    let mut choices: Vec<Choice> = Vec::new();
    let mut push = |m: &ModelInfo| {
        let id = guidy_core::downloads::stt_id(&m.id);
        if choices.iter().any(|c| c.id == id) {
            return;
        }
        let mut c = stt_choice(m);
        c.in_use = m.id == selected;
        c.recommended = recommended.as_deref() == Some(m.id.as_str());
        choices.push(c);
    };
    let find = |id: &str| inputs.stt_models.iter().find(|m| m.id == id);
    if let Some(m) = find(selected) {
        push(m);
    }
    if let Some(m) = recommended.as_deref().and_then(find) {
        push(m);
    }
    for m in inputs.stt_models.iter().filter(|m| m.is_downloaded) {
        push(m);
    }
    FeaturePlan {
        feature: Feature::Dictation,
        requirements: vec![Requirement {
            feature: Feature::Dictation,
            role: Role::SpeechModel,
            required: true,
            choices,
        }],
    }
}

/// The meeting transcription model meetings use now, or would download.
fn meeting_transcription(inputs: &Inputs) -> Requirement {
    let s = inputs.settings;
    let (id, in_use_ready) = match resolve_meeting_model_id(
        inputs.stt_models,
        s.meetings.meeting_model.as_deref(),
        Some(s.selected_model.as_str()),
    ) {
        Ok(id) => (id, true),
        Err(missing) if missing.known => (missing.model_id, false),
        Err(_) => (DEFAULT_MEETING_MODEL_ID.to_string(), false),
    };
    let mut choices = Vec::new();
    let model = inputs.stt_models.iter().find(|m| m.id == id).or_else(|| {
        inputs
            .stt_models
            .iter()
            .find(|m| m.id == DEFAULT_MEETING_MODEL_ID)
    });
    if let Some(m) = model {
        let mut c = stt_choice(m);
        c.in_use = true;
        c.ready = c.ready && in_use_ready;
        c.recommended = true;
        choices.push(c);
    }
    Requirement {
        feature: Feature::MeetingNotes,
        role: Role::MeetingTranscription,
        required: true,
        choices,
    }
}

/// The local LLM `provider`'s feature uses: the chosen one when on disk,
/// else the default, else any downloaded one (as the built-in runtime
/// does), else the chosen-or-default one to download.
fn effective_local_llm<'a>(inputs: &Inputs<'a>, chosen: &str) -> Option<&'a OtherModel> {
    let llms: Vec<&OtherModel> = inputs
        .other_models
        .iter()
        .filter(|m| m.kind == DownloadKind::Llm)
        .collect();
    let by_key = |key: &str| {
        llms.iter()
            .copied()
            .find(|m| DownloadKind::parse(&m.id).is_some_and(|(_, k)| k == key))
    };
    let chosen = by_key(chosen.trim());
    let default = llms.iter().copied().find(|m| m.is_default);
    chosen
        .filter(|m| m.installed)
        .or(default.filter(|m| m.installed))
        .or_else(|| llms.iter().copied().find(|m| m.installed))
        .or(chosen)
        .or(default)
}

fn llm_requirement(inputs: &Inputs, feature: Feature) -> Requirement {
    let s = inputs.settings;
    let (role, provider, core_feature, required) = match feature {
        Feature::PostProcessing => (
            Role::PostProcessing,
            s.post_process_provider_id.as_str(),
            guidy_core::llm::Feature::PostProcess,
            s.post_process_enabled,
        ),
        _ => (
            Role::Summaries,
            s.meetings.llm_provider_id.as_str(),
            guidy_core::llm::Feature::Meetings,
            true,
        ),
    };
    let chosen_local = guidy_core::llm::feature_model(s, core_feature, BUILTIN_LLM_PROVIDER_ID);
    let in_use_local = (provider == BUILTIN_LLM_PROVIDER_ID)
        .then(|| effective_local_llm(inputs, &chosen_local))
        .flatten()
        .map(|m| m.id.clone());

    let mut choices: Vec<Choice> = inputs
        .other_models
        .iter()
        .filter(|m| m.kind == DownloadKind::Llm)
        .map(|m| {
            let mut c = other_choice(m);
            c.in_use = in_use_local.as_deref() == Some(m.id.as_str());
            c
        })
        .collect();
    let mut ai = Choice {
        id: APPLE_INTELLIGENCE_PROVIDER_ID.to_string(),
        name: "Apple Intelligence".into(),
        kind: ChoiceKind::OnDevice,
        size_bytes: 0,
        download_bytes: 0,
        ready: inputs.apple_intelligence,
        available: inputs.apple_intelligence,
        recommended: false,
        in_use: provider == APPLE_INTELLIGENCE_PROVIDER_ID,
    };
    if !ai.available && !ai.in_use {
        ai.ready = false;
    }
    choices.push(ai);
    for p in s
        .post_process_providers
        .iter()
        .filter(|p| p.id != APPLE_INTELLIGENCE_PROVIDER_ID)
        .filter(|p| inputs.has_key(&p.id) || p.id == provider)
    {
        let ready = inputs.has_key(&p.id);
        choices.push(Choice {
            id: p.id.clone(),
            name: p.label.clone(),
            kind: ChoiceKind::Cloud,
            size_bytes: 0,
            download_bytes: 0,
            ready,
            available: true,
            recommended: false,
            in_use: p.id == provider,
        });
    }

    // Recommendation (see the module docs).
    let pick = choices
        .iter()
        .position(|c| c.in_use && c.ready)
        .or_else(|| {
            let roomy = inputs.ram_gb.is_none_or(|gb| gb >= 8);
            roomy
                .then(|| {
                    let default_id = inputs
                        .other_models
                        .iter()
                        .find(|m| m.kind == DownloadKind::Llm && m.is_default)
                        .map(|m| m.id.as_str());
                    choices
                        .iter()
                        .position(|c| Some(c.id.as_str()) == default_id && c.available)
                })
                .flatten()
        })
        .or_else(|| {
            choices
                .iter()
                .position(|c| c.kind == ChoiceKind::OnDevice && c.available)
        })
        .or_else(|| {
            choices
                .iter()
                .position(|c| c.kind == ChoiceKind::Cloud && c.ready)
        });
    if let Some(i) = pick {
        choices[i].recommended = true;
    }
    Requirement {
        feature,
        role,
        required,
        choices,
    }
}

fn speaker_choice(inputs: &Inputs, id: &str) -> Option<Choice> {
    inputs
        .other_models
        .iter()
        .find(|m| m.id == id)
        .map(other_choice)
}

fn meetings(inputs: &Inputs) -> FeaturePlan {
    let s = inputs.settings;
    let mut reqs = vec![meeting_transcription(inputs)];
    let speaker_id = guidy_core::meetings::processing::speaker_model::speaker_download_id();
    let mut speaker: Vec<Choice> = speaker_choice(inputs, speaker_id).into_iter().collect();
    for c in &mut speaker {
        c.in_use = true;
        c.recommended = true;
    }
    reqs.push(Requirement {
        feature: Feature::MeetingNotes,
        role: Role::SpeakerRecognition,
        required: true,
        choices: speaker,
    });
    let separation = s.meetings.speaker_separation_model.trim();
    if !separation.is_empty() && separation != "resnet34" {
        let id = DownloadKind::Separation.id(separation);
        if let Some(mut c) = speaker_choice(inputs, &id) {
            c.in_use = true;
            c.recommended = true;
            reqs.push(Requirement {
                feature: Feature::MeetingNotes,
                role: Role::SpeakerSeparation,
                required: false,
                choices: vec![c],
            });
        }
    }
    reqs.push(llm_requirement(inputs, Feature::MeetingNotes));
    FeaturePlan {
        feature: Feature::MeetingNotes,
        requirements: reqs,
    }
}

/// What `feature` needs with the current settings.
pub fn plan(inputs: &Inputs, feature: Feature) -> FeaturePlan {
    match feature {
        Feature::Dictation => dictation(inputs),
        Feature::MeetingNotes => meetings(inputs),
        Feature::PostProcessing => FeaturePlan {
            feature,
            requirements: vec![llm_requirement(inputs, Feature::PostProcessing)],
        },
    }
}

/// The enabled features that use the model with download-tracker id `id`
/// now (Models page badges; a delete warns about them).
pub fn required_by(inputs: &Inputs, id: &str) -> Vec<Feature> {
    let s = inputs.settings;
    let mut out = Vec::new();
    for feature in Feature::ALL {
        if !feature.enabled(s) {
            continue;
        }
        let p = plan(inputs, feature);
        let uses = p
            .requirements
            .iter()
            .filter(|r| r.role != Role::PostProcessing || s.post_process_enabled)
            .any(|r| r.in_use().is_some_and(|c| c.id == id));
        let extra = feature == Feature::MeetingNotes && meetings_extra_uses(s, id);
        if uses || extra {
            out.push(feature);
        }
    }
    out
}

/// Models meetings use beyond the plan's requirements: the live model and
/// the speaker-turn pair the user agreed to.
fn meetings_extra_uses(s: &AppSettings, id: &str) -> bool {
    use guidy_core::meetings::processing::speaker_model::{
        segmentation_download_id, separation_download_id,
    };
    use guidy_core::meetings::types::SpeakerModelId;
    let live = s
        .meetings
        .live_model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .is_some_and(|m| guidy_core::downloads::stt_id(m) == id);
    let turns = s.meetings.speaker_turn_models_consent
        && (id == segmentation_download_id()
            || id == separation_download_id(SpeakerModelId::TitanetLarge));
    live || turns
}

/// A ready-made set of downloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Setup {
    /// One speech model.
    Dictation,
    /// Meeting notes fully on this Mac: transcription, speakers and a local
    /// LLM for summaries.
    MeetingsOnThisMac,
    /// Meeting notes with summaries from a cloud provider: transcription and
    /// speakers on this Mac.
    MeetingsWithCloudAi,
}

impl Setup {
    pub const ALL: [Setup; 3] = [
        Self::Dictation,
        Self::MeetingsOnThisMac,
        Self::MeetingsWithCloudAi,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::MeetingsOnThisMac => "meetingsLocal",
            Self::MeetingsWithCloudAi => "meetingsCloud",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupPlan {
    pub setup: Setup,
    /// Every model the setup uses, ready or not (local choices only).
    pub models: Vec<Choice>,
    /// The cloud provider it uses, if any (`ready` = key saved).
    pub cloud: Option<Choice>,
}

impl SetupPlan {
    /// Download-tracker ids still to download.
    pub fn download_ids(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|c| !c.ready)
            .map(|c| c.id.clone())
            .collect()
    }

    pub fn download_bytes(&self) -> u64 {
        self.models.iter().map(|c| c.download_bytes).sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.models.iter().map(|c| c.size_bytes).sum()
    }

    /// Everything is on disk (and the cloud key saved).
    pub fn ready(&self) -> bool {
        self.models.iter().all(|c| c.ready) && self.cloud.as_ref().is_none_or(|c| c.ready)
    }

    /// A cloud setup without any provider key: the user has to add one.
    pub fn needs_key(&self) -> bool {
        self.cloud.as_ref().is_some_and(|c| !c.ready)
    }
}

/// The models (and cloud provider) a ready-made setup uses.
pub fn setup(inputs: &Inputs, which: Setup) -> SetupPlan {
    let mut models: Vec<Choice> = Vec::new();
    let mut add = |c: Option<&Choice>| {
        if let Some(c) = c {
            if c.kind == ChoiceKind::Local && !models.iter().any(|m| m.id == c.id) {
                models.push(c.clone());
            }
        }
    };
    let mut cloud = None;
    match which {
        Setup::Dictation => {
            let p = dictation(inputs);
            add(p.requirements[0].suggestion());
        }
        Setup::MeetingsOnThisMac | Setup::MeetingsWithCloudAi => {
            let p = meetings(inputs);
            for role in [Role::MeetingTranscription, Role::SpeakerRecognition] {
                add(p.requirement(role).and_then(Requirement::suggestion));
            }
            let summaries = p.requirement(Role::Summaries);
            let choices = summaries.map(|r| r.choices.as_slice()).unwrap_or(&[]);
            if which == Setup::MeetingsOnThisMac {
                let local = choices
                    .iter()
                    .filter(|c| c.kind == ChoiceKind::Local)
                    .find(|c| c.in_use && c.ready)
                    .or_else(|| {
                        choices
                            .iter()
                            .filter(|c| c.kind == ChoiceKind::Local)
                            .find(|c| c.ready)
                    })
                    .or_else(|| {
                        let default = inputs
                            .other_models
                            .iter()
                            .find(|m| m.kind == DownloadKind::Llm && m.is_default)?;
                        choices.iter().find(|c| c.id == default.id)
                    });
                add(local);
            } else {
                let cloud_choice = choices
                    .iter()
                    .filter(|c| c.kind == ChoiceKind::Cloud)
                    .find(|c| c.in_use && c.ready)
                    .or_else(|| {
                        choices
                            .iter()
                            .find(|c| c.kind == ChoiceKind::Cloud && c.ready)
                    })
                    .or_else(|| choices.iter().find(|c| c.kind == ChoiceKind::Cloud));
                cloud = Some(cloud_choice.cloned().unwrap_or_else(|| Choice {
                    id: String::new(),
                    name: String::new(),
                    kind: ChoiceKind::Cloud,
                    size_bytes: 0,
                    download_bytes: 0,
                    ready: false,
                    available: true,
                    recommended: true,
                    in_use: false,
                }));
            }
        }
    }
    SetupPlan {
        setup: which,
        models,
        cloud,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::models::test_model;

    const PARAKEET: &str = DEFAULT_MEETING_MODEL_ID;

    fn stt(id: &str, langs: &[&str], recommended: bool, mb: u64) -> ModelInfo {
        let mut m = test_model(id);
        m.supported_languages = langs.iter().map(|s| s.to_string()).collect();
        m.is_recommended = recommended;
        m.size_mb = mb;
        m
    }

    fn catalog() -> Vec<ModelInfo> {
        let mut english = stt("parakeet-unified-en", &["en"], true, 600);
        english.accuracy_score = 0.9;
        english.speed_score = 0.9;
        let mut v3 = stt(PARAKEET, &["en", "de", "fr", "es"], true, 740);
        v3.accuracy_score = 0.8;
        v3.speed_score = 0.8;
        let mut whisper = stt("whisper-small", &["en", "de", "ja"], false, 465);
        whisper.accuracy_score = 0.6;
        whisper.speed_score = 0.6;
        vec![english, v3, whisper]
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

    fn others() -> Vec<OtherModel> {
        let mut big = other(
            "llm:qwen3-8b-q4_k_m",
            DownloadKind::Llm,
            5_000_000_000,
            false,
        );
        big.recommended_min_ram_gb = Some(16);
        vec![
            other(
                "llm:qwen3-4b-instruct-2507-q4_k_m",
                DownloadKind::Llm,
                2_500_000_000,
                true,
            ),
            big,
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
        ]
    }

    struct World {
        stt: Vec<ModelInfo>,
        other: Vec<OtherModel>,
        settings: AppSettings,
        keys: Vec<String>,
        ai: bool,
        lang: &'static str,
        ram: Option<u32>,
    }

    impl World {
        fn new() -> Self {
            let mut settings = guidy_core::settings::get_default_settings();
            settings.selected_model = String::new();
            Self {
                stt: catalog(),
                other: others(),
                settings,
                keys: Vec::new(),
                ai: false,
                lang: "en-US",
                ram: Some(16),
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
                ram_gb: self.ram,
            }
        }

        fn install(&mut self, id: &str) {
            if let Some(m) = self.stt.iter_mut().find(|m| m.id == id) {
                m.is_downloaded = true;
            }
            if let Some(m) = self.other.iter_mut().find(|m| m.id == id) {
                m.installed = true;
            }
        }
    }

    fn ids(v: &[&Choice]) -> Vec<String> {
        v.iter().map(|c| c.id.clone()).collect()
    }

    #[test]
    fn dictation_recommends_a_model_for_the_system_language() {
        let mut w = World::new();
        let p = plan(&w.inputs(), Feature::Dictation);
        assert!(!p.ready());
        assert_eq!(ids(&p.downloads()), ["stt:parakeet-unified-en"]);
        assert_eq!(p.download_bytes(), 600 * MIB);

        w.lang = "de-DE";
        let p = plan(&w.inputs(), Feature::Dictation);
        assert_eq!(ids(&p.downloads()), [format!("stt:{PARAKEET}")]);

        // Only a non-recommended model speaks Japanese.
        w.lang = "ja";
        assert_eq!(recommended_stt(&w.inputs()).unwrap().id, "whisper-small");
        // Nothing speaks it: the first recommended one.
        w.lang = "xx";
        assert!(recommended_stt(&w.inputs()).unwrap().is_recommended);
    }

    #[test]
    fn dictation_is_ready_with_a_downloaded_selected_model() {
        let mut w = World::new();
        w.install("whisper-small");
        // Downloaded but not selected: suggested with nothing to download.
        let p = plan(&w.inputs(), Feature::Dictation);
        assert!(!p.ready());
        assert!(p.downloads().is_empty());
        assert_eq!(
            p.requirements[0].suggestion().unwrap().id,
            "stt:whisper-small"
        );
        w.settings.selected_model = "whisper-small".into();
        assert!(plan(&w.inputs(), Feature::Dictation).ready());
    }

    #[test]
    fn meetings_need_transcription_speakers_and_summaries() {
        let w = World::new();
        let p = plan(&w.inputs(), Feature::MeetingNotes);
        assert!(!p.ready());
        assert_eq!(
            ids(&p.downloads()),
            [
                format!("stt:{PARAKEET}"),
                "speaker:resnet34".into(),
                "llm:qwen3-4b-instruct-2507-q4_k_m".into(),
            ]
        );
        assert_eq!(p.download_bytes(), 740 * MIB + 26_530_550 + 2_500_000_000);
    }

    #[test]
    fn meetings_reuse_the_dictation_model_and_a_cloud_key() {
        let mut w = World::new();
        w.install("whisper-small");
        w.settings.selected_model = "whisper-small".into();
        w.install("speaker:resnet34");
        w.keys = vec!["openai".into()];
        w.settings.meetings.llm_provider_id = "openai".into();
        let p = plan(&w.inputs(), Feature::MeetingNotes);
        assert!(p.ready(), "{p:#?}");
        assert!(p.downloads().is_empty());
        let summaries = p.requirement(Role::Summaries).unwrap();
        assert_eq!(summaries.in_use().unwrap().kind, ChoiceKind::Cloud);
        assert!(summaries.in_use().unwrap().recommended);
    }

    #[test]
    fn summaries_fall_back_to_apple_intelligence_on_a_small_mac() {
        let mut w = World::new();
        w.ram = Some(4);
        w.ai = true;
        let p = plan(&w.inputs(), Feature::MeetingNotes);
        let r = p.requirement(Role::Summaries).unwrap();
        assert_eq!(r.recommended().unwrap().kind, ChoiceKind::OnDevice);
        // Without it: a cloud provider with a key.
        w.ai = false;
        w.keys = vec!["groq".into()];
        let p = plan(&w.inputs(), Feature::MeetingNotes);
        let r = p.requirement(Role::Summaries).unwrap();
        assert_eq!(r.recommended().unwrap().id, "groq");
    }

    #[test]
    fn a_downloaded_local_llm_is_used_when_the_chosen_one_is_missing() {
        let mut w = World::new();
        w.install("llm:qwen3-8b-q4_k_m");
        let p = plan(&w.inputs(), Feature::MeetingNotes);
        let r = p.requirement(Role::Summaries).unwrap();
        assert_eq!(r.in_use().unwrap().id, "llm:qwen3-8b-q4_k_m");
        assert!(r.satisfied());
    }

    #[test]
    fn post_processing_only_counts_when_enabled() {
        let mut w = World::new();
        w.settings.post_process_enabled = false;
        assert!(plan(&w.inputs(), Feature::PostProcessing).ready());
        w.settings.post_process_enabled = true;
        w.settings.post_process_provider_id = BUILTIN_LLM_PROVIDER_ID.into();
        let p = plan(&w.inputs(), Feature::PostProcessing);
        assert!(!p.ready());
        assert_eq!(ids(&p.downloads()), ["llm:qwen3-4b-instruct-2507-q4_k_m"]);
    }

    #[test]
    fn required_by_lists_the_enabled_features_using_a_model() {
        let mut w = World::new();
        w.install(PARAKEET);
        w.settings.selected_model = PARAKEET.into();
        w.install("speaker:resnet34");
        let stt = format!("stt:{PARAKEET}");
        assert_eq!(required_by(&w.inputs(), &stt), [Feature::Dictation]);
        assert!(required_by(&w.inputs(), "speaker:resnet34").is_empty());

        w.settings.meetings.enabled = true;
        assert_eq!(
            required_by(&w.inputs(), &stt),
            [Feature::Dictation, Feature::MeetingNotes]
        );
        assert_eq!(
            required_by(&w.inputs(), "speaker:resnet34"),
            [Feature::MeetingNotes]
        );
        // The default meetings provider is the built-in LLM.
        assert_eq!(
            required_by(&w.inputs(), "llm:qwen3-4b-instruct-2507-q4_k_m"),
            [Feature::MeetingNotes]
        );
        w.settings.post_process_enabled = true;
        w.settings.post_process_provider_id = BUILTIN_LLM_PROVIDER_ID.into();
        assert_eq!(
            required_by(&w.inputs(), "llm:qwen3-4b-instruct-2507-q4_k_m"),
            [Feature::MeetingNotes, Feature::PostProcessing]
        );
        assert!(required_by(&w.inputs(), "segmentation:pyannote").is_empty());
        w.settings.meetings.speaker_turn_models_consent = true;
        assert_eq!(
            required_by(&w.inputs(), "segmentation:pyannote"),
            [Feature::MeetingNotes]
        );
    }

    #[test]
    fn setups_list_their_downloads_without_duplicates() {
        let mut w = World::new();
        w.lang = "de";
        let dictation = setup(&w.inputs(), Setup::Dictation);
        assert_eq!(dictation.download_ids(), [format!("stt:{PARAKEET}")]);

        let local = setup(&w.inputs(), Setup::MeetingsOnThisMac);
        assert_eq!(
            local.download_ids(),
            [
                format!("stt:{PARAKEET}"),
                "speaker:resnet34".to_string(),
                "llm:qwen3-4b-instruct-2507-q4_k_m".to_string(),
            ]
        );
        assert!(local.cloud.is_none());

        let cloud = setup(&w.inputs(), Setup::MeetingsWithCloudAi);
        assert_eq!(cloud.download_ids().len(), 2);
        assert!(cloud.needs_key());
        assert!(!cloud.ready());
        w.keys = vec!["anthropic".into()];
        let cloud = setup(&w.inputs(), Setup::MeetingsWithCloudAi);
        assert!(!cloud.needs_key());
        assert_eq!(cloud.cloud.as_ref().unwrap().id, "anthropic");

        for id in local.download_ids() {
            w.install(id.trim_start_matches("stt:"));
        }
        let local = setup(&w.inputs(), Setup::MeetingsOnThisMac);
        assert!(local.ready());
        assert_eq!(local.download_bytes(), 0);
    }

    #[test]
    fn a_partial_download_counts_toward_what_is_left() {
        let mut w = World::new();
        w.stt[0].partial_size = 100 * MIB;
        let p = plan(&w.inputs(), Feature::Dictation);
        assert_eq!(p.download_bytes(), 500 * MIB);
    }
}
