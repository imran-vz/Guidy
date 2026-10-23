//! Meeting notes: record a meeting locally (mic + system audio), transcribe it
//! with named speakers, summarize it, and draft Linear tickets.
//!
//! Layout (see `docs/MEETING_NOTES_ARCH.md` for ownership and contracts):
//! - [`types`]: shared data types used by every layer and the UI.
//! - [`settings`]: the nested `meetings` settings object.
//! - [`store`]: encrypted SQLCipher `meetings.db`.
//! - [`audio_store`]: encrypted Opus track files + retention.
//! - [`native`]: macOS bridges (EventKit, VPIO mic, process tap, notifications).
//! - [`recorder`]: recording sessions, tray state, reminders, calendar prompts.
//! - [`live`]: live transcription of the recording in progress.
//! - [`processing`]: VAD -> transcription -> voiceprints -> clustering -> timeline.
//! - [`summarize`]: chunked LLM summaries and action items.
//! - [`manager`]: [`manager::MeetingsManager`], the facade the app calls.

pub mod audio_store;
pub mod live;
pub mod manager;
pub mod native;
pub mod processing;
pub mod recorder;
pub mod settings;
pub mod store;
pub mod summarize;
pub mod types;

/// Meeting features need Core Audio process taps (macOS 14.2+).
pub fn is_supported() -> bool {
    native::is_supported()
}
