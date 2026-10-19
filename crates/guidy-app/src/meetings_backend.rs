//! The UI's view of meeting notes.
//!
//! Every meeting command the UI invokes is a method here, one per
//! [`MeetingsManager`] method, in the same style as [`crate::backend::Backend`]:
//! synchronous (may block, so views call them from background tasks) and
//! returning `Result<_, String>`. Production uses [`CoreMeetingsBackend`]
//! (the `Arc<MeetingsManager>` registered in the app handle's state); UI tests
//! use [`fake::FakeMeetingsBackend`]. [`NoMeetings`] stands in when meeting
//! notes are not available at all (and in tests that don't need them).
//!
//! Ownership (docs/MEETING_NOTES_ARCH.md): G1 owns everything except the
//! `// -- AI + Linear (G2)` sections, which G2 fills. New trait methods need a
//! body in `CoreMeetingsBackend`, `NoMeetings` and the fake; a default body on
//! the trait (returning [`unavailable`]) keeps the others compiling.

use crate::logic::meetings::MeetingsInit;
pub use guidy_core::llm::ModelCatalog;
use guidy_core::meetings::manager::{
    MeetingsManager, NameSuggestion, PermissionReport, PersonRef, TicketResult,
};
use guidy_core::meetings::processing::SpeakerModelStatus;
use guidy_core::meetings::types::*;
use guidy_core::AppHandle;
use std::sync::Arc;

pub type MeetingsResult<T> = Result<T, String>;

pub fn unavailable() -> String {
    "Meeting notes are not available on this system".to_string()
}

/// Separator of a structured error string (never in normal error text).
const TAG: char = '\u{1f}';

/// The errors the UI treats specially. Backend methods keep returning
/// `String`; structured errors travel as a tagged string
/// ([`MeetingsError::encode`]) that [`MeetingsError::parse`] reads back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeetingsError {
    /// Meeting notes are not available (unsupported, or not started).
    Unavailable,
    /// The meeting transcription model is not downloaded (`known`) or not
    /// installed at all.
    ModelMissing {
        model_id: String,
        name: String,
        known: bool,
    },
    /// The speaker recognition model isn't downloaded (it downloads only
    /// from Meetings > Settings).
    SpeakerModelMissing,
    NoLinearKey,
    LinearUnauthorized,
    /// The job was cancelled ([`guidy_core::llm::CANCELLED`]): not an error
    /// to show.
    Cancelled,
    Other(String),
}

impl MeetingsError {
    pub fn from_anyhow(e: &anyhow::Error) -> Self {
        use guidy_core::linear::LinearError;
        use guidy_core::managers::transcription::meeting::MeetingModelMissing;
        use guidy_core::meetings::processing::SpeakerModelMissing;
        if guidy_core::llm::is_cancelled_error(e) {
            return Self::Cancelled;
        }
        for cause in e.chain() {
            if let Some(m) = cause.downcast_ref::<MeetingModelMissing>() {
                return Self::ModelMissing {
                    model_id: m.model_id.clone(),
                    name: m.model_name.clone(),
                    known: m.known,
                };
            }
            if cause.downcast_ref::<SpeakerModelMissing>().is_some() {
                return Self::SpeakerModelMissing;
            }
            match cause.downcast_ref::<LinearError>() {
                Some(LinearError::MissingApiKey) => return Self::NoLinearKey,
                Some(LinearError::Unauthorized) => return Self::LinearUnauthorized,
                _ => {}
            }
        }
        Self::Other(format!("{e:#}"))
    }

    /// The string form a `MeetingsResult` carries.
    pub fn encode(&self) -> String {
        match self {
            Self::Unavailable => unavailable(),
            Self::ModelMissing {
                model_id,
                name,
                known,
            } => format!("{TAG}model-missing{TAG}{model_id}{TAG}{name}{TAG}{known}"),
            Self::SpeakerModelMissing => format!("{TAG}speaker-model-missing"),
            Self::NoLinearKey => format!("{TAG}no-linear-key"),
            Self::LinearUnauthorized => format!("{TAG}linear-unauthorized"),
            Self::Cancelled => format!("{TAG}cancelled"),
            Self::Other(s) => s.clone(),
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == unavailable() {
            return Self::Unavailable;
        }
        let Some(rest) = s.strip_prefix(TAG) else {
            return Self::Other(s.to_string());
        };
        let parts: Vec<&str> = rest.split(TAG).collect();
        match parts.as_slice() {
            ["model-missing", id, name, known] => Self::ModelMissing {
                model_id: id.to_string(),
                name: name.to_string(),
                known: *known == "true",
            },
            ["speaker-model-missing"] => Self::SpeakerModelMissing,
            ["no-linear-key"] => Self::NoLinearKey,
            ["linear-unauthorized"] => Self::LinearUnauthorized,
            ["cancelled"] => Self::Cancelled,
            _ => Self::Other(s.to_string()),
        }
    }

    /// The localized message; `None` for [`Self::Other`] (show the raw text
    /// as detail under a localized title instead).
    pub fn localized(&self) -> Option<String> {
        use guidy_i18n::{t, t_args};
        Some(match self {
            Self::Unavailable => t("meetings.errors.unavailable"),
            Self::ModelMissing {
                name, known: true, ..
            } => t_args("meetings.errors.modelMissing", &[("name", name)]),
            Self::ModelMissing { name, .. } => {
                t_args("meetings.errors.modelUnknown", &[("name", name)])
            }
            Self::SpeakerModelMissing => t("meetings.errors.speakerModelMissing"),
            Self::NoLinearKey => t("meetings.errors.noLinearKey"),
            Self::LinearUnauthorized => t("meetings.errors.linearUnauthorized"),
            Self::Cancelled | Self::Other(_) => return None,
        })
    }

    /// Text for an inline error box: localized when known, else raw.
    pub fn message(&self) -> String {
        match self {
            Self::Other(s) => s.clone(),
            Self::Cancelled => guidy_core::llm::CANCELLED.to_string(),
            e => e.localized().unwrap_or_default(),
        }
    }
}

/// [`MeetingsError::message`] of a backend error string.
pub fn error_message(e: &str) -> String {
    MeetingsError::parse(e).message()
}

pub trait MeetingsBackend: Send + Sync + 'static {
    // -- support & permissions --------------------------------------------
    /// Meeting features exist on this system (macOS 14.2+). When false the
    /// Meetings section is hidden.
    fn is_supported(&self) -> bool;
    fn request_permissions(&self) -> MeetingsResult<PermissionReport>;
    fn permission_status(&self) -> PermissionReport;

    // -- calendar -----------------------------------------------------------
    fn upcoming_events(&self) -> MeetingsResult<Vec<CalendarEvent>>;

    // -- recording ----------------------------------------------------------
    fn recorder_state(&self) -> RecorderState;
    fn start_recording(&self, calendar_event_id: Option<String>) -> MeetingsResult<MeetingId>;
    fn stop_recording(&self) -> MeetingsResult<Option<MeetingId>>;
    /// 16 kHz mono f32 samples of one track between `t0_ms` and `t1_ms`.
    fn segment_audio(
        &self,
        meeting_id: MeetingId,
        track: Track,
        t0_ms: i64,
        t1_ms: i64,
    ) -> MeetingsResult<Vec<f32>>;

    // -- meetings & people ------------------------------------------------
    fn list_meetings(&self) -> MeetingsResult<Vec<Meeting>>;
    fn get_meeting(&self, id: MeetingId) -> MeetingsResult<Option<Meeting>>;
    fn rename_meeting(&self, id: MeetingId, title: &str) -> MeetingsResult<()>;
    fn delete_meeting(&self, id: MeetingId) -> MeetingsResult<()>;
    fn segments(&self, id: MeetingId) -> MeetingsResult<Vec<Segment>>;
    fn speakers(&self, id: MeetingId) -> MeetingsResult<Vec<MeetingSpeaker>>;
    fn list_people(&self) -> MeetingsResult<Vec<Person>>;
    fn rename_person(
        &self,
        id: PersonId,
        display_name: &str,
        email: Option<&str>,
    ) -> MeetingsResult<()>;
    fn merge_people(&self, from: PersonId, into: PersonId) -> MeetingsResult<()>;
    fn delete_person(&self, id: PersonId) -> MeetingsResult<()>;

    // -- processing & speakers --------------------------------------------
    fn process_meeting(&self, id: MeetingId) -> MeetingsResult<()>;
    fn cancel_processing(&self, id: MeetingId) -> MeetingsResult<()>;
    fn processing_progress(&self, id: MeetingId) -> Option<ProcessingProgress>;
    fn speaker_suggestions(&self, speaker_id: SpeakerId) -> MeetingsResult<Vec<NameSuggestion>>;
    fn assign_speaker(&self, speaker_id: SpeakerId, person: PersonRef) -> MeetingsResult<PersonId>;
    fn confirm_speaker(&self, speaker_id: SpeakerId) -> MeetingsResult<()>;

    /// The speaker-embedding model (downloads on first use).
    fn speaker_model_status(&self) -> SpeakerModelStatus {
        SpeakerModelStatus::Missing
    }
    /// Start downloading the speaker model; progress arrives as
    /// `meetings://speaker-model-status`.
    fn download_speaker_model(&self) -> MeetingsResult<()> {
        Err(unavailable())
    }

    // -- startup ------------------------------------------------------------
    /// Where the meetings manager's startup is.
    fn init_status(&self) -> MeetingsInit {
        MeetingsInit::Ready
    }
    /// Start the manager again after a failed start.
    fn retry_init(&self) {}
    /// Move unreadable meeting data aside (nothing is deleted), make a new
    /// data key and start the manager again.
    fn reset_data(&self) -> MeetingsResult<()> {
        Err(unavailable())
    }

    // -- AI + Linear (G2) ---------------------------------------------------
    // Default bodies keep `NoMeetings` (and any other impl) compiling.

    // Summaries & action items.
    fn summary(&self, _id: MeetingId) -> MeetingsResult<Option<StoredSummary>> {
        Ok(None)
    }
    /// `provider_override`: `(provider_id, model)`, else the meetings model
    /// from settings.
    fn summarize(
        &self,
        _id: MeetingId,
        _provider_override: Option<(String, String)>,
    ) -> MeetingsResult<StoredSummary> {
        Err(unavailable())
    }
    fn update_summary(&self, _summary: &StoredSummary) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn action_items(&self, _id: MeetingId) -> MeetingsResult<Vec<ActionItem>> {
        Ok(Vec::new())
    }
    fn update_action_item(&self, _item: &ActionItem) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn add_action_item(&self, _meeting_id: MeetingId, _text: &str) -> MeetingsResult<ActionItem> {
        Err(unavailable())
    }
    fn delete_action_item(&self, _id: ActionItemId) -> MeetingsResult<()> {
        Err(unavailable())
    }
    /// Stop a running summary. False when none was running.
    fn cancel_summarize(&self, _id: MeetingId) -> bool {
        false
    }
    fn is_summarizing(&self, _id: MeetingId) -> bool {
        false
    }

    // Linear.
    fn set_linear_api_key(&self, _key: &str) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn has_linear_api_key(&self) -> bool {
        false
    }
    /// The authenticated Linear user's name.
    fn test_linear(&self) -> MeetingsResult<String> {
        Err(unavailable())
    }
    fn linear_teams(&self) -> MeetingsResult<Vec<guidy_core::linear::Team>> {
        Ok(Vec::new())
    }
    fn linear_projects(&self, _team_id: &str) -> MeetingsResult<Vec<guidy_core::linear::Project>> {
        Ok(Vec::new())
    }
    fn linear_users(&self) -> MeetingsResult<Vec<guidy_core::linear::User>> {
        Ok(Vec::new())
    }
    fn linear_labels(&self, _team_id: &str) -> MeetingsResult<Vec<guidy_core::linear::Label>> {
        Ok(Vec::new())
    }
    fn draft_tickets(&self, _meeting_id: MeetingId) -> MeetingsResult<Vec<ActionItem>> {
        Err(unavailable())
    }
    fn create_tickets(&self, _ids: &[ActionItemId]) -> MeetingsResult<Vec<TicketResult>> {
        Err(unavailable())
    }

    // LLM providers (meetings model, BYOK).
    /// Check a provider's URL + key with a small request. Returns a short
    /// human-readable success message (e.g. the number of models). `feature`
    /// picks which configured model is probed.
    fn llm_test_connection(
        &self,
        _provider_id: &str,
        _feature: guidy_core::llm::Feature,
    ) -> MeetingsResult<String> {
        Err(unavailable())
    }
    /// Model ids the provider offers (local: downloaded built-in models).
    fn llm_models(&self, _provider_id: &str) -> MeetingsResult<Vec<String>> {
        Ok(Vec::new())
    }
    /// A cloud provider's models described by the models.dev registry.
    fn llm_model_catalog(&self, _provider_id: &str) -> MeetingsResult<ModelCatalog> {
        Err(unavailable())
    }
    /// Download the models.dev registry now. Returns when it was fetched
    /// (unix seconds).
    fn refresh_llm_registry(&self) -> MeetingsResult<u64> {
        Err(unavailable())
    }
    /// Built-in llama.cpp models (catalog + download state).
    fn local_llm_models(&self) -> MeetingsResult<Vec<LocalLlmModel>> {
        Ok(Vec::new())
    }
    fn download_local_llm_model(&self, _id: &str) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn cancel_local_llm_download(&self, _id: &str) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn delete_local_llm_model(&self, _id: &str) -> MeetingsResult<()> {
        Err(unavailable())
    }
    /// Open a URL in the default browser: only Linear pages and the
    /// System Settings privacy panes
    /// ([`crate::logic::meetings::is_allowed_external_url`]).
    fn open_url(&self, _url: &str) -> MeetingsResult<()> {
        Err(unavailable())
    }
}

/// A built-in llama.cpp model as the meetings AI settings show it.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalLlmModel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_mb: u64,
    /// The default model (~4B).
    pub is_default: bool,
    /// Recommended minimum RAM in GB (16 for the 8B model).
    pub recommended_ram_gb: Option<u32>,
    /// This Mac has the recommended RAM (or there is no minimum).
    pub fits_this_mac: bool,
    pub downloaded: bool,
    /// 0.0..=1.0 while downloading.
    pub download_progress: Option<f32>,
}

pub type SharedMeetingsBackend = Arc<dyn MeetingsBackend>;

/// The live backend: forwards to the `Arc<MeetingsManager>` in app state.
/// The manager may be absent (unsupported system, startup failure): reads
/// then return empty values and commands return an error.
pub struct CoreMeetingsBackend {
    app: AppHandle,
}

impl CoreMeetingsBackend {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    pub fn manager(&self) -> Option<Arc<MeetingsManager>> {
        self.app
            .try_state::<Arc<MeetingsManager>>()
            .map(|s| Arc::clone(&*s))
    }

    fn m(&self) -> MeetingsResult<Arc<MeetingsManager>> {
        self.manager().ok_or_else(unavailable)
    }
}

fn err(e: anyhow::Error) -> String {
    MeetingsError::from_anyhow(&e).encode()
}

impl MeetingsBackend for CoreMeetingsBackend {
    fn is_supported(&self) -> bool {
        guidy_core::meetings::is_supported()
    }
    // Permissions don't need the manager: turning meeting notes on asks for
    // them while the manager is still starting.
    fn request_permissions(&self) -> MeetingsResult<PermissionReport> {
        if !self.is_supported() {
            return Err(unavailable());
        }
        guidy_core::meetings::native::request_permissions().map_err(err)
    }
    fn permission_status(&self) -> PermissionReport {
        guidy_core::meetings::native::permission_status()
    }

    fn upcoming_events(&self) -> MeetingsResult<Vec<CalendarEvent>> {
        match self.manager() {
            Some(m) => m.upcoming_events().map_err(err),
            None => Ok(Vec::new()),
        }
    }

    fn recorder_state(&self) -> RecorderState {
        self.manager()
            .map(|m| m.recorder_state())
            .unwrap_or_default()
    }
    fn start_recording(&self, calendar_event_id: Option<String>) -> MeetingsResult<MeetingId> {
        self.m()?.start_recording(calendar_event_id).map_err(err)
    }
    fn stop_recording(&self) -> MeetingsResult<Option<MeetingId>> {
        self.m()?.stop_recording().map_err(err)
    }
    fn segment_audio(
        &self,
        meeting_id: MeetingId,
        track: Track,
        t0_ms: i64,
        t1_ms: i64,
    ) -> MeetingsResult<Vec<f32>> {
        self.m()?
            .segment_audio(meeting_id, track, t0_ms, t1_ms)
            .map_err(err)
    }

    fn list_meetings(&self) -> MeetingsResult<Vec<Meeting>> {
        match self.manager() {
            Some(m) => m.list_meetings().map_err(err),
            None => Ok(Vec::new()),
        }
    }
    fn get_meeting(&self, id: MeetingId) -> MeetingsResult<Option<Meeting>> {
        self.m()?.get_meeting(id).map_err(err)
    }
    fn rename_meeting(&self, id: MeetingId, title: &str) -> MeetingsResult<()> {
        self.m()?.rename_meeting(id, title).map_err(err)
    }
    fn delete_meeting(&self, id: MeetingId) -> MeetingsResult<()> {
        self.m()?.delete_meeting(id).map_err(err)
    }
    fn segments(&self, id: MeetingId) -> MeetingsResult<Vec<Segment>> {
        self.m()?.segments(id).map_err(err)
    }
    fn speakers(&self, id: MeetingId) -> MeetingsResult<Vec<MeetingSpeaker>> {
        self.m()?.speakers(id).map_err(err)
    }
    fn list_people(&self) -> MeetingsResult<Vec<Person>> {
        match self.manager() {
            Some(m) => m.list_people().map_err(err),
            None => Ok(Vec::new()),
        }
    }
    fn rename_person(
        &self,
        id: PersonId,
        display_name: &str,
        email: Option<&str>,
    ) -> MeetingsResult<()> {
        self.m()?
            .rename_person(id, display_name, email)
            .map_err(err)
    }
    fn merge_people(&self, from: PersonId, into: PersonId) -> MeetingsResult<()> {
        self.m()?.merge_people(from, into).map_err(err)
    }
    fn delete_person(&self, id: PersonId) -> MeetingsResult<()> {
        self.m()?.delete_person(id).map_err(err)
    }

    fn process_meeting(&self, id: MeetingId) -> MeetingsResult<()> {
        self.m()?.process_meeting(id).map_err(err)
    }
    fn cancel_processing(&self, id: MeetingId) -> MeetingsResult<()> {
        self.m()?.cancel_processing(id).map_err(err)
    }
    fn processing_progress(&self, id: MeetingId) -> Option<ProcessingProgress> {
        self.manager()?.processing_progress(id)
    }
    fn speaker_suggestions(&self, speaker_id: SpeakerId) -> MeetingsResult<Vec<NameSuggestion>> {
        self.m()?.speaker_suggestions(speaker_id).map_err(err)
    }
    fn assign_speaker(&self, speaker_id: SpeakerId, person: PersonRef) -> MeetingsResult<PersonId> {
        self.m()?.assign_speaker(speaker_id, person).map_err(err)
    }
    fn confirm_speaker(&self, speaker_id: SpeakerId) -> MeetingsResult<()> {
        self.m()?.confirm_speaker(speaker_id).map_err(err)
    }
    fn speaker_model_status(&self) -> SpeakerModelStatus {
        guidy_core::meetings::processing::speaker_model_status(&self.app)
    }
    fn download_speaker_model(&self) -> MeetingsResult<()> {
        guidy_core::meetings::processing::start_speaker_model_download(&self.app);
        Ok(())
    }

    fn init_status(&self) -> MeetingsInit {
        MeetingsInit::from_core(guidy_core::meetings_init_status(&self.app))
    }
    fn retry_init(&self) {
        guidy_core::retry_init_meetings(&self.app)
    }
    fn reset_data(&self) -> MeetingsResult<()> {
        guidy_core::reset_meetings_data(&self.app).map_err(err)
    }

    // -- AI + Linear (G2) ---------------------------------------------------
    fn summary(&self, id: MeetingId) -> MeetingsResult<Option<StoredSummary>> {
        self.m()?.summary(id).map_err(err)
    }
    fn cancel_summarize(&self, id: MeetingId) -> bool {
        self.manager().is_some_and(|m| m.cancel_summarize(id))
    }
    fn is_summarizing(&self, id: MeetingId) -> bool {
        self.manager().is_some_and(|m| m.is_summarizing(id))
    }
    fn summarize(
        &self,
        id: MeetingId,
        provider_override: Option<(String, String)>,
    ) -> MeetingsResult<StoredSummary> {
        self.m()?.summarize(id, provider_override).map_err(err)
    }
    fn update_summary(&self, summary: &StoredSummary) -> MeetingsResult<()> {
        self.m()?.update_summary(summary).map_err(err)
    }
    fn action_items(&self, id: MeetingId) -> MeetingsResult<Vec<ActionItem>> {
        self.m()?.action_items(id).map_err(err)
    }
    fn update_action_item(&self, item: &ActionItem) -> MeetingsResult<()> {
        self.m()?.update_action_item(item).map_err(err)
    }
    fn add_action_item(&self, meeting_id: MeetingId, text: &str) -> MeetingsResult<ActionItem> {
        self.m()?.add_action_item(meeting_id, text).map_err(err)
    }
    fn delete_action_item(&self, id: ActionItemId) -> MeetingsResult<()> {
        self.m()?.delete_action_item(id).map_err(err)
    }

    fn set_linear_api_key(&self, key: &str) -> MeetingsResult<()> {
        self.m()?.set_linear_api_key(key).map_err(err)
    }
    fn has_linear_api_key(&self) -> bool {
        self.manager().is_some_and(|m| m.has_linear_api_key())
    }
    fn test_linear(&self) -> MeetingsResult<String> {
        self.m()?.test_linear().map_err(err)
    }
    fn linear_teams(&self) -> MeetingsResult<Vec<guidy_core::linear::Team>> {
        self.m()?.linear_teams().map_err(err)
    }
    fn linear_projects(&self, team_id: &str) -> MeetingsResult<Vec<guidy_core::linear::Project>> {
        self.m()?.linear_projects(team_id).map_err(err)
    }
    fn linear_users(&self) -> MeetingsResult<Vec<guidy_core::linear::User>> {
        self.m()?.linear_users().map_err(err)
    }
    fn linear_labels(&self, team_id: &str) -> MeetingsResult<Vec<guidy_core::linear::Label>> {
        self.m()?.linear_labels(team_id).map_err(err)
    }
    fn draft_tickets(&self, meeting_id: MeetingId) -> MeetingsResult<Vec<ActionItem>> {
        self.m()?.draft_tickets(meeting_id).map_err(err)
    }
    fn create_tickets(&self, ids: &[ActionItemId]) -> MeetingsResult<Vec<TicketResult>> {
        self.m()?.create_tickets(ids).map_err(err)
    }

    fn llm_test_connection(
        &self,
        provider_id: &str,
        feature: guidy_core::llm::Feature,
    ) -> MeetingsResult<String> {
        g2_core::test_connection(&self.app, provider_id, feature)
    }
    fn llm_models(&self, provider_id: &str) -> MeetingsResult<Vec<String>> {
        g2_core::list_models(&self.app, provider_id)
    }
    fn llm_model_catalog(&self, provider_id: &str) -> MeetingsResult<ModelCatalog> {
        guidy_core::llm::model_catalog(&self.app, provider_id).map_err(err)
    }
    fn refresh_llm_registry(&self) -> MeetingsResult<u64> {
        guidy_core::llm::registry::refresh(&self.app)
            .map(|r| r.fetched_at.unwrap_or_default())
            .map_err(err)
    }
    fn local_llm_models(&self) -> MeetingsResult<Vec<LocalLlmModel>> {
        g2_core::local_models(&self.app)
    }
    fn download_local_llm_model(&self, id: &str) -> MeetingsResult<()> {
        g2_core::download_local(&self.app, id)
    }
    fn cancel_local_llm_download(&self, id: &str) -> MeetingsResult<()> {
        g2_core::cancel_local(&self.app, id)
    }
    fn delete_local_llm_model(&self, id: &str) -> MeetingsResult<()> {
        g2_core::delete_local(&self.app, id)
    }
    fn open_url(&self, url: &str) -> MeetingsResult<()> {
        if !crate::logic::meetings::is_allowed_external_url(url) {
            log::warn!("Refusing to open a non-Linear URL from meeting notes");
            return Err(format!("Refusing to open {url}"));
        }
        self.app.opener().open_url(url, None)
    }
}

/// G2: the `CoreMeetingsBackend` LLM methods, over `guidy_core::llm`.
mod g2_core {
    use super::{LocalLlmModel, MeetingsResult};
    use guidy_core::llm::local_models::{LocalModel, LocalModels};
    use guidy_core::AppHandle;

    fn err(e: anyhow::Error) -> String {
        super::err(e)
    }

    pub fn test_connection(
        app: &AppHandle,
        provider_id: &str,
        feature: guidy_core::llm::Feature,
    ) -> MeetingsResult<String> {
        guidy_core::llm::test_connection(app, provider_id, feature).map_err(err)
    }

    pub fn list_models(app: &AppHandle, provider_id: &str) -> MeetingsResult<Vec<String>> {
        guidy_core::llm::list_models(app, provider_id).map_err(err)
    }

    pub fn to_view(m: LocalModel) -> LocalLlmModel {
        let download_progress = m.downloading.then(|| {
            if m.size_bytes == 0 {
                0.0
            } else {
                (m.partial_bytes as f64 / m.size_bytes as f64).clamp(0.0, 1.0) as f32
            }
        });
        LocalLlmModel {
            id: m.id,
            name: m.name,
            description: m.description,
            size_mb: m.size_bytes / (1024 * 1024),
            is_default: m.is_default,
            recommended_ram_gb: m.recommended_min_ram_gb,
            fits_this_mac: m.fits_this_mac,
            downloaded: m.downloaded,
            download_progress,
        }
    }

    pub fn local_models(app: &AppHandle) -> MeetingsResult<Vec<LocalLlmModel>> {
        Ok(LocalModels::new(app)
            .map_err(err)?
            .list()
            .into_iter()
            .map(to_view)
            .collect())
    }

    pub fn download_local(app: &AppHandle, id: &str) -> MeetingsResult<()> {
        LocalModels::new(app)
            .map_err(err)?
            .start_download(id)
            .map_err(err)
    }

    pub fn cancel_local(app: &AppHandle, id: &str) -> MeetingsResult<()> {
        LocalModels::new(app).map_err(err)?.cancel_download(id);
        Ok(())
    }

    pub fn delete_local(app: &AppHandle, id: &str) -> MeetingsResult<()> {
        LocalModels::new(app).map_err(err)?.delete(id).map_err(err)
    }
}

/// Meeting notes are unavailable: hidden section, empty reads, failing
/// commands.
pub struct NoMeetings;

impl MeetingsBackend for NoMeetings {
    fn is_supported(&self) -> bool {
        false
    }
    fn download_speaker_model(&self) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn request_permissions(&self) -> MeetingsResult<PermissionReport> {
        Err(unavailable())
    }
    fn permission_status(&self) -> PermissionReport {
        PermissionReport::default()
    }
    fn upcoming_events(&self) -> MeetingsResult<Vec<CalendarEvent>> {
        Ok(Vec::new())
    }
    fn recorder_state(&self) -> RecorderState {
        RecorderState::Idle
    }
    fn start_recording(&self, _: Option<String>) -> MeetingsResult<MeetingId> {
        Err(unavailable())
    }
    fn stop_recording(&self) -> MeetingsResult<Option<MeetingId>> {
        Err(unavailable())
    }
    fn segment_audio(&self, _: MeetingId, _: Track, _: i64, _: i64) -> MeetingsResult<Vec<f32>> {
        Err(unavailable())
    }
    fn list_meetings(&self) -> MeetingsResult<Vec<Meeting>> {
        Ok(Vec::new())
    }
    fn get_meeting(&self, _: MeetingId) -> MeetingsResult<Option<Meeting>> {
        Ok(None)
    }
    fn rename_meeting(&self, _: MeetingId, _: &str) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn delete_meeting(&self, _: MeetingId) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn segments(&self, _: MeetingId) -> MeetingsResult<Vec<Segment>> {
        Ok(Vec::new())
    }
    fn speakers(&self, _: MeetingId) -> MeetingsResult<Vec<MeetingSpeaker>> {
        Ok(Vec::new())
    }
    fn list_people(&self) -> MeetingsResult<Vec<Person>> {
        Ok(Vec::new())
    }
    fn rename_person(&self, _: PersonId, _: &str, _: Option<&str>) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn merge_people(&self, _: PersonId, _: PersonId) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn delete_person(&self, _: PersonId) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn process_meeting(&self, _: MeetingId) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn cancel_processing(&self, _: MeetingId) -> MeetingsResult<()> {
        Err(unavailable())
    }
    fn processing_progress(&self, _: MeetingId) -> Option<ProcessingProgress> {
        None
    }
    fn speaker_suggestions(&self, _: SpeakerId) -> MeetingsResult<Vec<NameSuggestion>> {
        Ok(Vec::new())
    }
    fn assign_speaker(&self, _: SpeakerId, _: PersonRef) -> MeetingsResult<PersonId> {
        Err(unavailable())
    }
    fn confirm_speaker(&self, _: SpeakerId) -> MeetingsResult<()> {
        Err(unavailable())
    }

    // -- AI + Linear (G2) ---------------------------------------------------
}

#[cfg(test)]
mod error_tests {
    use super::*;
    use guidy_core::managers::transcription::meeting::MeetingModelMissing;

    #[test]
    fn typed_errors_survive_the_string_round_trip() {
        let missing = anyhow::Error::new(MeetingModelMissing {
            model_id: "org/parakeet/q8.gguf".into(),
            model_name: "Parakeet".into(),
            known: true,
        })
        .context("process meeting 4");
        let e = MeetingsError::from_anyhow(&missing);
        assert_eq!(
            e,
            MeetingsError::ModelMissing {
                model_id: "org/parakeet/q8.gguf".into(),
                name: "Parakeet".into(),
                known: true,
            }
        );
        assert_eq!(MeetingsError::parse(&e.encode()), e);
        let speaker = anyhow::Error::new(guidy_core::meetings::processing::SpeakerModelMissing)
            .context("process meeting 4");
        assert_eq!(
            MeetingsError::parse(&err(speaker)),
            MeetingsError::SpeakerModelMissing
        );
        let linear = anyhow::Error::new(guidy_core::linear::LinearError::MissingApiKey);
        assert_eq!(
            MeetingsError::parse(&err(linear)),
            MeetingsError::NoLinearKey
        );
        assert_eq!(
            MeetingsError::parse(&unavailable()),
            MeetingsError::Unavailable
        );
        assert_eq!(
            MeetingsError::parse("disk full"),
            MeetingsError::Other("disk full".into())
        );
        let cancelled = anyhow::anyhow!(guidy_core::llm::CANCELLED);
        assert_eq!(
            MeetingsError::parse(&err(cancelled)),
            MeetingsError::Cancelled
        );
        // Only the job's own error: text that merely mentions it isn't.
        assert_ne!(
            MeetingsError::parse(&err(anyhow::anyhow!("cancelled: disk full"))),
            MeetingsError::Cancelled
        );
    }

    #[test]
    fn known_errors_are_localized_and_others_kept_raw() {
        guidy_i18n::set_language("en");
        let e = MeetingsError::ModelMissing {
            model_id: "x".into(),
            name: "Parakeet".into(),
            known: true,
        };
        assert!(error_message(&e.encode()).contains("Parakeet"));
        assert!(!error_message(&e.encode()).contains('\u{1f}'));
        assert_eq!(
            error_message(&MeetingsError::NoLinearKey.encode()),
            guidy_i18n::t("meetings.errors.noLinearKey")
        );
        assert_eq!(error_message("boom"), "boom");
        assert_eq!(MeetingsError::Other("boom".into()).localized(), None);
    }
}

/// Recording fake for UI tests (same style as `testing::FakeBackend`).
#[cfg(test)]
pub mod fake {
    use super::*;
    use guidy_core::meetings::manager::SuggestionSource;
    use parking_lot::Mutex;
    use std::collections::HashMap;

    pub struct FakeMeetingsData {
        pub supported: bool,
        pub permissions: PermissionReport,
        pub recorder: RecorderState,
        pub upcoming: Vec<CalendarEvent>,
        pub meetings: Vec<Meeting>,
        pub segments: HashMap<MeetingId, Vec<Segment>>,
        pub speakers: HashMap<MeetingId, Vec<MeetingSpeaker>>,
        pub people: Vec<Person>,
        /// Extra suggestions returned before the known people.
        pub attendees: Vec<NameSuggestion>,
        pub progress: HashMap<MeetingId, ProcessingProgress>,
        pub next_id: i64,
        pub speaker_model: SpeakerModelStatus,
        pub init: MeetingsInit,
        /// `process_meeting` fails with this (encoded) error.
        pub fail_process: Option<String>,
        /// G2: summaries, action items, Linear, LLM.
        pub ai: FakeAiData,
    }

    impl Default for FakeMeetingsData {
        fn default() -> Self {
            Self {
                supported: true,
                permissions: PermissionReport::default(),
                recorder: RecorderState::Idle,
                upcoming: Vec::new(),
                meetings: Vec::new(),
                segments: HashMap::new(),
                speakers: HashMap::new(),
                people: Vec::new(),
                attendees: Vec::new(),
                progress: HashMap::new(),
                next_id: 1000,
                speaker_model: SpeakerModelStatus::Ready,
                init: MeetingsInit::Ready,
                fail_process: None,
                ai: FakeAiData::default(),
            }
        }
    }

    /// Records calls as `"name arg1 arg2"` strings.
    pub struct FakeMeetingsBackend {
        pub calls: Mutex<Vec<String>>,
        pub data: Mutex<FakeMeetingsData>,
    }

    impl FakeMeetingsBackend {
        pub fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                data: Mutex::new(FakeMeetingsData::default()),
            })
        }

        fn rec(&self, call: impl Into<String>) {
            self.calls.lock().push(call.into());
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().clone()
        }

        pub fn called(&self, prefix: &str) -> bool {
            self.calls.lock().iter().any(|c| c.starts_with(prefix))
        }

        fn speaker_mut<R>(
            &self,
            speaker_id: SpeakerId,
            f: impl FnOnce(&mut MeetingSpeaker) -> R,
        ) -> MeetingsResult<R> {
            let mut d = self.data.lock();
            d.speakers
                .values_mut()
                .flat_map(|v| v.iter_mut())
                .find(|s| s.id == speaker_id)
                .map(f)
                .ok_or_else(|| format!("no speaker {speaker_id}"))
        }
    }

    impl MeetingsBackend for FakeMeetingsBackend {
        fn is_supported(&self) -> bool {
            self.data.lock().supported
        }
        fn request_permissions(&self) -> MeetingsResult<PermissionReport> {
            self.rec("request_permissions");
            Ok(self.data.lock().permissions.clone())
        }
        fn permission_status(&self) -> PermissionReport {
            self.data.lock().permissions.clone()
        }
        fn upcoming_events(&self) -> MeetingsResult<Vec<CalendarEvent>> {
            Ok(self.data.lock().upcoming.clone())
        }
        fn recorder_state(&self) -> RecorderState {
            self.data.lock().recorder.clone()
        }
        fn start_recording(&self, calendar_event_id: Option<String>) -> MeetingsResult<MeetingId> {
            self.rec(format!(
                "start_recording {}",
                calendar_event_id.as_deref().unwrap_or("none")
            ));
            let mut d = self.data.lock();
            d.next_id += 1;
            let id = d.next_id;
            let title = calendar_event_id
                .as_ref()
                .and_then(|e| d.upcoming.iter().find(|u| &u.id == e))
                .map(|e| e.title.clone())
                .unwrap_or_else(|| "Meeting".into());
            d.recorder = RecorderState::Recording {
                meeting_id: id,
                title,
                started_at: 0,
                scheduled_end_at: None,
            };
            Ok(id)
        }
        fn stop_recording(&self) -> MeetingsResult<Option<MeetingId>> {
            self.rec("stop_recording");
            let mut d = self.data.lock();
            let id = match &d.recorder {
                RecorderState::Recording { meeting_id, .. } => Some(*meeting_id),
                RecorderState::Idle => None,
            };
            d.recorder = RecorderState::Idle;
            Ok(id)
        }
        fn segment_audio(
            &self,
            meeting_id: MeetingId,
            track: Track,
            t0_ms: i64,
            t1_ms: i64,
        ) -> MeetingsResult<Vec<f32>> {
            self.rec(format!(
                "segment_audio {meeting_id} {} {t0_ms} {t1_ms}",
                track.as_str()
            ));
            Ok(vec![0.0; ((t1_ms - t0_ms).max(0) * 16) as usize])
        }
        fn list_meetings(&self) -> MeetingsResult<Vec<Meeting>> {
            self.rec("list_meetings");
            Ok(self.data.lock().meetings.clone())
        }
        fn get_meeting(&self, id: MeetingId) -> MeetingsResult<Option<Meeting>> {
            Ok(self
                .data
                .lock()
                .meetings
                .iter()
                .find(|m| m.id == id)
                .cloned())
        }
        fn rename_meeting(&self, id: MeetingId, title: &str) -> MeetingsResult<()> {
            self.rec(format!("rename_meeting {id} {title}"));
            if let Some(m) = self.data.lock().meetings.iter_mut().find(|m| m.id == id) {
                m.title = title.to_string();
            }
            Ok(())
        }
        fn delete_meeting(&self, id: MeetingId) -> MeetingsResult<()> {
            self.rec(format!("delete_meeting {id}"));
            self.data.lock().meetings.retain(|m| m.id != id);
            Ok(())
        }
        fn segments(&self, id: MeetingId) -> MeetingsResult<Vec<Segment>> {
            Ok(self
                .data
                .lock()
                .segments
                .get(&id)
                .cloned()
                .unwrap_or_default())
        }
        fn speakers(&self, id: MeetingId) -> MeetingsResult<Vec<MeetingSpeaker>> {
            Ok(self
                .data
                .lock()
                .speakers
                .get(&id)
                .cloned()
                .unwrap_or_default())
        }
        fn list_people(&self) -> MeetingsResult<Vec<Person>> {
            Ok(self.data.lock().people.clone())
        }
        fn rename_person(
            &self,
            id: PersonId,
            display_name: &str,
            email: Option<&str>,
        ) -> MeetingsResult<()> {
            self.rec(format!(
                "rename_person {id} {display_name} {}",
                email.unwrap_or("-")
            ));
            if let Some(p) = self.data.lock().people.iter_mut().find(|p| p.id == id) {
                p.display_name = display_name.to_string();
                p.email = email.map(str::to_string);
            }
            Ok(())
        }
        fn merge_people(&self, from: PersonId, into: PersonId) -> MeetingsResult<()> {
            self.rec(format!("merge_people {from} {into}"));
            let mut d = self.data.lock();
            let moved = d
                .people
                .iter()
                .find(|p| p.id == from)
                .map(|p| p.voiceprint_count)
                .unwrap_or(0);
            d.people.retain(|p| p.id != from);
            if let Some(p) = d.people.iter_mut().find(|p| p.id == into) {
                p.voiceprint_count += moved;
            }
            Ok(())
        }
        fn delete_person(&self, id: PersonId) -> MeetingsResult<()> {
            self.rec(format!("delete_person {id}"));
            self.data.lock().people.retain(|p| p.id != id);
            Ok(())
        }
        fn process_meeting(&self, id: MeetingId) -> MeetingsResult<()> {
            self.rec(format!("process_meeting {id}"));
            if let Some(e) = self.data.lock().fail_process.clone() {
                return Err(e);
            }
            if let Some(m) = self.data.lock().meetings.iter_mut().find(|m| m.id == id) {
                m.status = MeetingStatus::Processing;
            }
            Ok(())
        }
        fn cancel_processing(&self, id: MeetingId) -> MeetingsResult<()> {
            self.rec(format!("cancel_processing {id}"));
            Ok(())
        }
        fn processing_progress(&self, id: MeetingId) -> Option<ProcessingProgress> {
            self.data.lock().progress.get(&id).cloned()
        }
        fn speaker_suggestions(
            &self,
            speaker_id: SpeakerId,
        ) -> MeetingsResult<Vec<NameSuggestion>> {
            self.rec(format!("speaker_suggestions {speaker_id}"));
            let d = self.data.lock();
            let mut v = d.attendees.clone();
            v.extend(d.people.iter().map(|p| NameSuggestion {
                display_name: p.display_name.clone(),
                email: p.email.clone(),
                person_id: Some(p.id),
                source: SuggestionSource::KnownPerson,
                similarity: None,
            }));
            Ok(v)
        }
        fn assign_speaker(
            &self,
            speaker_id: SpeakerId,
            person: PersonRef,
        ) -> MeetingsResult<PersonId> {
            let pid = match &person {
                PersonRef::Existing(pid) => {
                    self.rec(format!("assign_speaker {speaker_id} existing {pid}"));
                    *pid
                }
                PersonRef::New {
                    display_name,
                    email,
                } => {
                    self.rec(format!(
                        "assign_speaker {speaker_id} new {display_name} {}",
                        email.as_deref().unwrap_or("-")
                    ));
                    let mut d = self.data.lock();
                    d.next_id += 1;
                    let id = d.next_id;
                    d.people
                        .push(person_fixture(id, display_name, email.as_deref()));
                    id
                }
            };
            self.speaker_mut(speaker_id, |s| {
                s.person_id = Some(pid);
                s.confirmed = true;
            })?;
            Ok(pid)
        }
        fn confirm_speaker(&self, speaker_id: SpeakerId) -> MeetingsResult<()> {
            self.rec(format!("confirm_speaker {speaker_id}"));
            self.speaker_mut(speaker_id, |s| s.confirmed = true)
        }
        fn speaker_model_status(&self) -> SpeakerModelStatus {
            self.data.lock().speaker_model.clone()
        }
        fn download_speaker_model(&self) -> MeetingsResult<()> {
            self.rec("download_speaker_model");
            Ok(())
        }
        fn init_status(&self) -> MeetingsInit {
            self.data.lock().init.clone()
        }
        fn retry_init(&self) {
            self.rec("retry_init");
        }
        fn reset_data(&self) -> MeetingsResult<()> {
            self.rec("reset_data");
            let mut d = self.data.lock();
            d.init = MeetingsInit::Ready;
            Ok(())
        }
        fn cancel_summarize(&self, id: MeetingId) -> bool {
            self.rec(format!("cancel_summarize {id}"));
            !self.data.lock().ai.cancel_refused
        }
        fn is_summarizing(&self, id: MeetingId) -> bool {
            self.data.lock().ai.summarizing.contains(&id)
        }

        // -- AI + Linear (G2) -----------------------------------------------
        fn summary(&self, id: MeetingId) -> MeetingsResult<Option<StoredSummary>> {
            Ok(self.data.lock().ai.summaries.get(&id).cloned())
        }
        fn summarize(
            &self,
            id: MeetingId,
            provider_override: Option<(String, String)>,
        ) -> MeetingsResult<StoredSummary> {
            let by = provider_override
                .map(|(p, m)| format!("{p}/{m}"))
                .unwrap_or_else(|| "default".into());
            self.rec(format!("summarize {id} {by}"));
            let mut d = self.data.lock();
            if let Some(e) = d.ai.fail_summarize.clone() {
                return Err(e);
            }
            let s = StoredSummary {
                meeting_id: id,
                title: "Generated".into(),
                short_summary: "A short summary.".into(),
                decisions: vec!["Ship it".into()],
                topics: vec!["Release".into()],
                generated_by: by,
                generated_at: 1,
            };
            d.ai.summaries.insert(id, s.clone());
            Ok(s)
        }
        fn update_summary(&self, summary: &StoredSummary) -> MeetingsResult<()> {
            self.rec(format!(
                "update_summary {} {} decisions={}",
                summary.meeting_id,
                summary.title,
                summary.decisions.join("|")
            ));
            self.data
                .lock()
                .ai
                .summaries
                .insert(summary.meeting_id, summary.clone());
            Ok(())
        }
        fn action_items(&self, id: MeetingId) -> MeetingsResult<Vec<ActionItem>> {
            Ok(self
                .data
                .lock()
                .ai
                .items
                .iter()
                .filter(|i| i.meeting_id == id)
                .cloned()
                .collect())
        }
        fn update_action_item(&self, item: &ActionItem) -> MeetingsResult<()> {
            self.rec(format!(
                "update_action_item {} {} {}",
                item.id,
                item.ticket_state.as_str(),
                item.text
            ));
            let mut d = self.data.lock();
            if let Some(i) = d.ai.items.iter_mut().find(|i| i.id == item.id) {
                *i = item.clone();
            }
            Ok(())
        }
        fn add_action_item(&self, meeting_id: MeetingId, text: &str) -> MeetingsResult<ActionItem> {
            self.rec(format!("add_action_item {meeting_id} {text}"));
            let mut d = self.data.lock();
            d.next_id += 1;
            let item = action_item_fixture(d.next_id, meeting_id, text);
            d.ai.items.push(item.clone());
            Ok(item)
        }
        fn delete_action_item(&self, id: ActionItemId) -> MeetingsResult<()> {
            self.rec(format!("delete_action_item {id}"));
            self.data.lock().ai.items.retain(|i| i.id != id);
            Ok(())
        }

        fn set_linear_api_key(&self, key: &str) -> MeetingsResult<()> {
            self.rec(format!("set_linear_api_key {key}"));
            self.data.lock().ai.linear_key = (!key.is_empty()).then(|| key.to_string());
            Ok(())
        }
        fn has_linear_api_key(&self) -> bool {
            self.data.lock().ai.linear_key.is_some()
        }
        fn test_linear(&self) -> MeetingsResult<String> {
            self.rec("test_linear");
            match self.data.lock().ai.linear_key {
                Some(_) => Ok("Ada Lovelace".into()),
                None => Err("no key".into()),
            }
        }
        fn linear_teams(&self) -> MeetingsResult<Vec<guidy_core::linear::Team>> {
            Ok(vec![guidy_core::linear::Team {
                id: "team-1".into(),
                key: "ENG".into(),
                name: "Engineering".into(),
            }])
        }
        fn linear_projects(
            &self,
            _team_id: &str,
        ) -> MeetingsResult<Vec<guidy_core::linear::Project>> {
            Ok(vec![guidy_core::linear::Project {
                id: "proj-1".into(),
                name: "Launch".into(),
            }])
        }
        fn linear_users(&self) -> MeetingsResult<Vec<guidy_core::linear::User>> {
            Ok(vec![guidy_core::linear::User {
                id: "user-1".into(),
                name: "Ada Lovelace".into(),
                display_name: "ada".into(),
                email: Some("ada@example.com".into()),
            }])
        }
        fn linear_labels(&self, _team_id: &str) -> MeetingsResult<Vec<guidy_core::linear::Label>> {
            Ok(vec![guidy_core::linear::Label {
                id: "label-1".into(),
                name: "Bug".into(),
            }])
        }
        fn draft_tickets(&self, meeting_id: MeetingId) -> MeetingsResult<Vec<ActionItem>> {
            self.rec(format!("draft_tickets {meeting_id}"));
            let mut d = self.data.lock();
            for i in d.ai.items.iter_mut().filter(|i| i.meeting_id == meeting_id) {
                if i.ticket.title.is_empty() {
                    i.ticket.title = i.text.clone();
                    i.ticket.description = format!("> {}", i.source_quote);
                    i.ticket.team_id = Some("team-1".into());
                }
            }
            Ok(d.ai
                .items
                .iter()
                .filter(|i| i.meeting_id == meeting_id)
                .cloned()
                .collect())
        }
        fn create_tickets(&self, ids: &[ActionItemId]) -> MeetingsResult<Vec<TicketResult>> {
            let list: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
            self.rec(format!("create_tickets {}", list.join(",")));
            let mut d = self.data.lock();
            let mut out = Vec::new();
            for id in ids {
                let Some(i) = d.ai.items.iter_mut().find(|i| i.id == *id) else {
                    continue;
                };
                let already = i.linear_issue_id.is_some();
                if !already {
                    let ident = format!("ENG-{id}");
                    i.linear_issue_id = Some(format!("issue-{id}"));
                    i.linear_issue_url = Some(format!("https://linear.app/x/issue/{ident}"));
                    i.linear_issue_identifier = Some(ident);
                    i.ticket_state = TicketState::Created;
                }
                out.push(TicketResult {
                    action_item_id: *id,
                    result: Ok(i.linear_issue_identifier.clone().unwrap_or_default()),
                    already_existed: already,
                });
            }
            Ok(out)
        }

        fn llm_test_connection(
            &self,
            provider_id: &str,
            feature: guidy_core::llm::Feature,
        ) -> MeetingsResult<String> {
            self.rec(format!("llm_test_connection {provider_id} {feature:?}"));
            let test = self.data.lock().ai.llm_test.clone();
            match test {
                Some(f) => f(provider_id),
                None => Ok("Connected".into()),
            }
        }
        fn llm_model_catalog(&self, provider_id: &str) -> MeetingsResult<ModelCatalog> {
            self.rec(format!("llm_model_catalog {provider_id}"));
            Ok(self.data.lock().ai.catalog.clone())
        }
        fn refresh_llm_registry(&self) -> MeetingsResult<u64> {
            self.rec("refresh_llm_registry".to_string());
            Ok(1)
        }
        fn llm_models(&self, provider_id: &str) -> MeetingsResult<Vec<String>> {
            self.rec(format!("llm_models {provider_id}"));
            let d = self.data.lock();
            match &d.ai.fail_llm_models {
                Some(e) => Err(e.clone()),
                None => Ok(d.ai.llm_models.clone()),
            }
        }
        fn local_llm_models(&self) -> MeetingsResult<Vec<LocalLlmModel>> {
            Ok(self.data.lock().ai.local_models.clone())
        }
        fn download_local_llm_model(&self, id: &str) -> MeetingsResult<()> {
            self.rec(format!("download_local_llm_model {id}"));
            Ok(())
        }
        fn cancel_local_llm_download(&self, id: &str) -> MeetingsResult<()> {
            self.rec(format!("cancel_local_llm_download {id}"));
            Ok(())
        }
        fn delete_local_llm_model(&self, id: &str) -> MeetingsResult<()> {
            self.rec(format!("delete_local_llm_model {id}"));
            Ok(())
        }
        fn open_url(&self, url: &str) -> MeetingsResult<()> {
            self.rec(format!("open_url {url}"));
            Ok(())
        }
    }

    /// G2 fake state: summaries, action items, Linear key, LLM models.
    #[derive(Default)]
    pub struct FakeAiData {
        pub summaries: HashMap<MeetingId, StoredSummary>,
        pub items: Vec<ActionItem>,
        pub linear_key: Option<String>,
        pub fail_summarize: Option<String>,
        /// Meetings `is_summarizing` reports as running.
        pub summarizing: Vec<MeetingId>,
        /// `cancel_summarize` returns false (the job is already storing).
        pub cancel_refused: bool,
        pub llm_models: Vec<String>,
        /// What `llm_model_catalog` returns.
        pub catalog: ModelCatalog,
        /// `llm_models` fails with this message.
        pub fail_llm_models: Option<String>,
        /// What `llm_test_connection` returns for a provider (default
        /// "Connected").
        #[allow(clippy::type_complexity)]
        pub llm_test: Option<Arc<dyn Fn(&str) -> MeetingsResult<String> + Send + Sync>>,
        pub local_models: Vec<LocalLlmModel>,
    }

    pub fn action_item_fixture(id: ActionItemId, meeting_id: MeetingId, text: &str) -> ActionItem {
        ActionItem {
            id,
            meeting_id,
            text: text.into(),
            owner_person_id: None,
            owner_name: None,
            owner_is_user: false,
            due: None,
            source_quote: format!("I'll {text}"),
            source_t_ms: Some(65_000),
            ticket: TicketDraft::default(),
            ticket_state: TicketState::Draft,
            linear_issue_id: None,
            linear_issue_identifier: None,
            linear_issue_url: None,
        }
    }

    // -- fixtures -------------------------------------------------------------

    pub fn meeting_fixture(id: MeetingId, title: &str, status: MeetingStatus) -> Meeting {
        Meeting {
            id,
            title: title.into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at: 1_700_000_000_000 + id * 3_600_000,
            ended_at: Some(1_700_000_000_000 + id * 3_600_000 + 25 * 60_000),
            mic_audio_path: Some(format!("{id}-mic.opus")),
            system_audio_path: Some(format!("{id}-system.opus")),
            status,
            error: None,
            processing_stage: None,
        }
    }

    pub fn person_fixture(id: PersonId, name: &str, email: Option<&str>) -> Person {
        Person {
            id,
            display_name: name.into(),
            email: email.map(str::to_string),
            created_at: 1_700_000_000_000,
            voiceprint_count: 3,
        }
    }

    pub fn speaker_fixture(
        id: SpeakerId,
        meeting_id: MeetingId,
        label: &str,
        person_id: Option<PersonId>,
        confirmed: bool,
    ) -> MeetingSpeaker {
        MeetingSpeaker {
            id,
            meeting_id,
            cluster_label: label.into(),
            person_id,
            match_confidence: person_id.map(|_| 0.55),
            confirmed,
            is_user: label == "me",
        }
    }

    pub fn segment_fixture(
        id: i64,
        meeting_id: MeetingId,
        speaker_id: SpeakerId,
        t0_ms: i64,
        text: &str,
    ) -> Segment {
        Segment {
            id,
            meeting_id,
            track: Track::System,
            t0_ms,
            t1_ms: t0_ms + 2_000,
            text: text.into(),
            words: Vec::new(),
            speaker_id: Some(speaker_id),
        }
    }
}
