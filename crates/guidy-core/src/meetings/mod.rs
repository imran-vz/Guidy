//! Meeting notes: record a meeting locally (mic + system audio), transcribe it
//! with named speakers, summarize it, and draft Linear tickets.
//!
//! Layout (see `docs/MEETING_NOTES_ARCH.md` for ownership and contracts):
//! - [`types`]: shared data types used by every layer and the UI.
//! - [`calls`]: Zoom, Meet or WhatsApp calls in progress (to offer notes).
//! - [`settings`]: the nested `meetings` settings object.
//! - [`store`]: plain SQLite `meetings.db`.
//! - [`audio_store`]: Opus track files + retention.
//! - [`legacy`]: one-time conversion of data earlier versions encrypted.
//! - [`native`]: macOS bridges (EventKit, mic, process tap, echo cancellation, notifications).
//! - [`recorder`]: recording sessions, tray state, reminders, calendar prompts.
//! - [`live`]: live transcription of the recording in progress.
//! - [`processing`]: VAD -> transcription -> voiceprints -> clustering -> timeline.
//! - [`summarize`]: chunked LLM summaries and action items.
//! - [`titles`]: titles and descriptions generated from a summary.
//! - [`search`]: full-text search across meetings.
//! - [`manager`]: [`manager::MeetingsManager`], the facade the app calls.

pub mod audio_store;
pub mod calls;
pub mod legacy;
pub mod live;
pub mod manager;
pub mod native;
pub mod processing;
pub mod recorder;
pub mod search;
pub mod settings;
pub mod store;
pub mod summarize;
pub mod titles;
pub mod types;

/// Meeting features need Core Audio process taps (macOS 14.2+).
pub fn is_supported() -> bool {
    native::is_supported()
}
