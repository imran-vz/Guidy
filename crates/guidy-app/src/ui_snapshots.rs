//! Offscreen PNG snapshots of the real windows (main window pages, meetings,
//! onboarding, overlay, recording widget) in light and dark, rendered by
//! GPUI's headless Metal renderer with real CoreText shaping and the app's
//! own assets (Geist fonts, icons, themes). Nothing touches the user's app
//! data: every backend is a fake populated by the builders below.
//!
//! Opt-in (`#[ignore]`), see docs/UI_SNAPSHOTS.md:
//!
//! ```sh
//! cargo test -p guidy-app ui_snapshots -- --ignored --nocapture
//! GUIDY_SNAPSHOT_FILTER=meetings cargo test -p guidy-app ui_snapshots -- --ignored --nocapture
//! ```
//!
//! Output: `target/ui-snapshots/<name>-<light|dark>.png`.

use crate::logic::flows::OnboardingStep;
use crate::state::AppState;
use crate::testing::{
    action_item_fixture, history_entry, person_fixture, FakeBackend, FakeMeetingsBackend,
};
use crate::ui::design::dp;
use crate::ui::main_window::{MainWindow, Section};
use gpui_kit::component::TitleBar;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, size, AnyWindowHandle, AppContext as _, Bounds, HeadlessAppContext, Pixels, Point, Size,
    WindowBounds, WindowOptions,
};
use guidy_core::managers::model::{ModelInfo, ModelSource, RetiredModel};
use guidy_core::meetings::types::{
    Meeting, MeetingSpeaker, MeetingStatus, NoteSection, Person, PersonId, Segment, StoredSummary,
    Track,
};
use guidy_core::settings::Theme as ThemeSetting;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// -- sizes ------------------------------------------------------------------

/// The main window's default size (`windows::MAIN_DEFAULT_*` at `dp`).
fn default_size() -> Size<Pixels> {
    use crate::windows::{MAIN_DEFAULT_HEIGHT, MAIN_DEFAULT_WIDTH};
    size(dp(MAIN_DEFAULT_WIDTH), dp(MAIN_DEFAULT_HEIGHT))
}

/// The main window's minimum size: every page must still work at it.
fn min_size() -> Size<Pixels> {
    use crate::windows::{MAIN_HEIGHT, MAIN_WIDTH};
    size(dp(MAIN_WIDTH), dp(MAIN_HEIGHT))
}

/// Default width, tall enough that a page shows content below the fold
/// (pages scroll inside the window, so a taller window shows more).
fn tall_size() -> Size<Pixels> {
    use crate::windows::MAIN_DEFAULT_WIDTH;
    size(dp(MAIN_DEFAULT_WIDTH), px(2400.))
}

fn wide_size() -> Size<Pixels> {
    size(px(1440.), px(900.))
}

// -- the harness --------------------------------------------------------------

/// One app (fresh globals per snapshot) on the headless Metal renderer.
struct Harness {
    cx: HeadlessAppContext,
    backend: Arc<FakeBackend>,
    meetings: Arc<FakeMeetingsBackend>,
    theme: ThemeSetting,
    /// Real time to wait between the first frame and the capture, for
    /// entrance animations (GPUI's `Animation` runs on the wall clock, not
    /// the test clock).
    wall_wait: Duration,
}

impl Harness {
    fn new(theme: ThemeSetting) -> Self {
        let cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(crate::assets::AppAssets),
            gpui_kit::platform::current_headless_renderer,
        );
        let backend = FakeBackend::new();
        backend.with_settings(|s| s.theme = theme);
        Self {
            cx,
            backend,
            meetings: FakeMeetingsBackend::new(),
            theme,
            wall_wait: Duration::ZERO,
        }
    }

    /// Install the app state (after the fakes are filled in).
    fn install(&mut self) {
        let backend = self.backend.clone();
        let meetings = self.meetings.clone();
        let theme = self.theme;
        self.cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            crate::ui::init(cx);
            guidy_i18n::set_language("en");
            AppState::install_with_meetings(backend, meetings, cx);
            crate::ui::overlay::OverlayState::install(cx);
            crate::theme::apply_setting(theme, cx);
            // Day groups and "next event" read this, not the wall clock.
            cx.set_global(crate::ui::pages::meetings::MeetingsClock(BASE_MS));
        });
        self.settle();
    }

    fn open<V: gpui_kit::Render + 'static>(
        &mut self,
        size: Size<Pixels>,
        titlebar: bool,
        build: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::Context<V>) -> V + 'static,
    ) -> (AnyWindowHandle, gpui_kit::Entity<V>) {
        let base = if titlebar {
            TitleBar::window_options()
        } else {
            WindowOptions::default()
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size,
            })),
            focus: false,
            show: false,
            ..base
        };
        let (handle, view) = self
            .cx
            .update(|cx| gpui_kit::open_window(options, cx, |w, cx| cx.new(|cx| build(w, cx))))
            .expect("open snapshot window");
        self.settle();
        (handle, view)
    }

    fn open_main(&mut self, size: Size<Pixels>) -> (AnyWindowHandle, gpui_kit::Entity<MainWindow>) {
        self.open(size, true, MainWindow::new)
    }

    /// Run queued work and timers (the permissions step's 300 ms poll, the
    /// toast/what's-new delays) without letting wall-clock animations
    /// matter: they render at whatever phase they're at.
    fn settle(&mut self) {
        for _ in 0..4 {
            self.cx.run_until_parked();
            self.cx.advance_clock(Duration::from_millis(500));
        }
        self.cx.run_until_parked();
    }

    fn with_window(
        &mut self,
        w: AnyWindowHandle,
        f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App),
    ) {
        self.cx
            .update_window(w, |_, window, cx| {
                window.render_frame(cx);
                f(window, cx);
            })
            .unwrap();
        self.settle();
    }

    /// Render settled frames, then write the window's pixels as a PNG.
    /// Returns the image size in device pixels.
    fn capture(&mut self, w: AnyWindowHandle, path: &std::path::Path) -> (u32, u32) {
        // Two frames: the first lays out lazily loaded content (images,
        // list measurements), the second paints it settled.
        for _ in 0..2 {
            self.cx
                .update_window(w, |_, window, cx| window.render_frame(cx))
                .unwrap();
            self.settle();
        }
        if !self.wall_wait.is_zero() {
            std::thread::sleep(self.wall_wait);
        }
        self.cx
            .update_window(w, |_, window, cx| window.render_frame(cx))
            .unwrap();
        let image = self
            .cx
            .capture_screenshot(w)
            .expect("headless Metal rendering must be available");
        image.save(path).expect("write png");
        (image.width(), image.height())
    }
}

// -- fake data builders ---------------------------------------------------------

/// Fixed timestamps (2026-03-10 09:00 local-ish, ms) so dates are stable.
const BASE_MS: i64 = 1_773_133_200_000;

fn model(
    id: &str,
    name: &str,
    description: &str,
    size_mb: u64,
    accuracy: f32,
    speed: f32,
) -> ModelInfo {
    let mut m = crate::logic::models::test_model(id);
    m.name = name.into();
    m.description = description.into();
    m.size_mb = size_mb;
    m.accuracy_score = accuracy;
    m.speed_score = speed;
    m.source = ModelSource::HuggingFace {
        repo_id: format!("handy-computer/{id}-gguf"),
        revision: "main".into(),
    };
    m.supported_languages = WHISPER_LANGUAGES.iter().map(|c| c.to_string()).collect();
    m
}

/// Fixture language lists that agree with the fixture descriptions.
const WHISPER_LANGUAGES: [&str; 20] = [
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it",
    "id", "hi", "fi", "vi",
];
const EUROPEAN_LANGUAGES: [&str; 25] = [
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt",
    "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
];

fn languages(codes: &[&str]) -> Vec<String> {
    codes.iter().map(|c| c.to_string()).collect()
}

fn models() -> Vec<ModelInfo> {
    let mut parakeet = model(
        "parakeet-tdt-0.6b-v3",
        "Parakeet V3",
        "Fast and accurate. Supports 25 European languages.",
        478,
        0.82,
        0.92,
    );
    parakeet.supported_languages = languages(&EUROPEAN_LANGUAGES);
    parakeet.is_downloaded = true;
    parakeet.is_recommended = true;
    parakeet.supports_streaming = true;
    let mut small = model(
        "small",
        "Whisper Small",
        "Fast and fairly accurate.",
        465,
        0.60,
        0.85,
    );
    small.is_downloaded = true;
    small.supports_translation = true;
    let mut turbo = model(
        "turbo",
        "Whisper Turbo",
        "Balanced accuracy and speed.",
        1549,
        0.80,
        0.40,
    );
    turbo.is_downloading = true;
    turbo.partial_size = 620 * 1024 * 1024;
    let mut large = model(
        "large",
        "Whisper Large",
        "Good accuracy, but slow.",
        1031,
        0.85,
        0.30,
    );
    large.supports_translation = true;
    let mut moonshine = model(
        "moonshine-base",
        "Moonshine Base",
        "Very fast, English only.",
        58,
        0.55,
        0.97,
    );
    moonshine.supported_languages = languages(&["en"]);
    let mut breeze = model(
        "breeze-asr",
        "Breeze ASR",
        "Optimized for Taiwanese Mandarin. Code-switching support.",
        1030,
        0.85,
        0.35,
    );
    breeze.supported_languages = languages(&["zh", "en"]);
    vec![parakeet, small, turbo, large, moonshine, breeze]
}

fn history() -> Vec<guidy_core::managers::history::HistoryEntry> {
    let texts = [
        "Can you send me the slides from this morning's roadmap review before lunch?",
        "Remind me to book the venue for the offsite and check the dietary list with Priya.",
        "The build is green again. The flaky test was a race in the download progress handler.",
        "Dear Martin, thanks for the quick turnaround on the contract. Two small notes below.",
        "Add oat milk, lemons, basil and the good parmesan to the shopping list.",
        "Let's ship the onboarding rework behind a flag and collect feedback for a week.",
        "Quick idea: the meeting summary could link each decision to the moment it was made.",
    ];
    texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let mut e = history_entry(100 - i as i64, text);
            // Newest first, a few hours apart.
            e.timestamp = BASE_MS / 1000 - i as i64 * 3 * 3600;
            e.saved = i == 1;
            if i == 3 {
                e.post_processed_text = Some(
                    "Dear Martin,\n\nThanks for the quick turnaround on the contract. Two small notes below."
                        .into(),
                );
                e.post_process_prompt = Some("Email".into());
                e.post_process_requested = true;
            }
            e
        })
        .collect()
}

fn usage() -> guidy_core::llm::usage::UsageReport {
    use guidy_core::llm::usage::*;
    UsageReport {
        models: vec![
            ModelSpend {
                provider_id: "anthropic".into(),
                model: "claude-sonnet-4-5".into(),
                calls: 42,
                input_tokens: 812_000,
                output_tokens: 64_300,
                total_usd: 3.41,
                avg_usd: Some(0.081),
                max_usd: Some(0.32),
                last_used_ms: BASE_MS,
                ..Default::default()
            },
            ModelSpend {
                provider_id: "openai".into(),
                model: "gpt-4.1-mini".into(),
                calls: 118,
                input_tokens: 240_000,
                output_tokens: 31_000,
                total_usd: 0.27,
                avg_usd: Some(0.0023),
                max_usd: Some(0.01),
                last_used_ms: BASE_MS - 86_400_000,
                ..Default::default()
            },
        ],
        features: vec![
            FeatureSpend {
                feature: UsageFeature::MeetingSummary,
                calls: 40,
                total_usd: 3.2,
            },
            FeatureSpend {
                feature: UsageFeature::Dictation,
                calls: 112,
                total_usd: 0.31,
            },
            FeatureSpend {
                feature: UsageFeature::MeetingTitle,
                calls: 8,
                total_usd: 0.17,
            },
        ],
        total_usd: 3.68,
        calls: 160,
        last_30_days_usd: 1.94,
        local: LocalUsageReport {
            models: vec![LocalModelUsage {
                kind: LocalKind::Transcription,
                model: "parakeet-tdt-0.6b-v3".into(),
                runs: 1_284,
                audio_ms: 5 * 3_600_000,
                words: 41_200,
                compute_ms: 9 * 60_000,
                timed_audio_ms: 5 * 3_600_000,
                ..Default::default()
            }],
            features: vec![],
            runs: 1_284,
            audio_ms: 5 * 3_600_000,
            words: 41_200,
            tokens: 0,
            compute_ms: 9 * 60_000,
        },
    }
}

fn meeting(
    id: i64,
    title: &str,
    minutes_ago: i64,
    length_min: i64,
    status: MeetingStatus,
) -> Meeting {
    let start = BASE_MS - minutes_ago * 60_000;
    Meeting {
        id,
        title: title.into(),
        calendar_event_id: None,
        calendar_event: None,
        started_at: start,
        ended_at: Some(start + length_min * 60_000),
        mic_audio_path: Some(format!("{id}-mic.opus")),
        system_audio_path: Some(format!("{id}-system.opus")),
        status,
        error: None,
        processing_stage: None,
        description: None,
        title_source: Default::default(),
        has_summary: false,
    }
}

fn speaker(
    id: i64,
    meeting_id: i64,
    label: &str,
    person: Option<i64>,
    is_user: bool,
) -> MeetingSpeaker {
    MeetingSpeaker {
        id,
        meeting_id,
        cluster_label: label.into(),
        person_id: person,
        match_confidence: person.map(|_| 0.86),
        confirmed: person.is_some() || is_user,
        is_user,
    }
}

/// The open meeting (id 1): a roadmap review with three speakers.
const DETAIL_ID: i64 = 1;

fn transcript(meeting_id: i64) -> Vec<Segment> {
    // (speaker id, text): 10 = me, 11 = Ada, 12 = Bob
    let lines: [(i64, &str); 14] = [
        (
            11,
            "Okay, let's get started. First on the list is the onboarding rework.",
        ),
        (
            10,
            "Sounds good. Where did we land on the permissions step?",
        ),
        (
            11,
            "We merged microphone and accessibility into one screen with a live check.",
        ),
        (
            12,
            "The drop-off there went from twenty percent to about eight in the beta.",
        ),
        (
            10,
            "That's a big win. Did anyone complain about the model download size?",
        ),
        (
            12,
            "A few people on slow connections. We could default to the smaller Parakeet build.",
        ),
        (
            11,
            "Let's do that, and show the download estimate before they commit.",
        ),
        (
            10,
            "Agreed. Next, meeting notes. The summaries are good but the titles are bland.",
        ),
        (
            12,
            "I can try generating the title from the decisions instead of the first minute.",
        ),
        (
            11,
            "Please do. And the speaker naming still asks too often.",
        ),
        (
            10,
            "We could remember voices across meetings once someone confirms a name.",
        ),
        (
            12,
            "That's mostly there already, it just needs a confidence threshold we trust.",
        ),
        (
            11,
            "Great. Bob, can you have a draft of the threshold change by Thursday?",
        ),
        (
            12,
            "Yes, Thursday works. I'll send it to both of you for review.",
        ),
    ];
    lines
        .iter()
        .enumerate()
        .map(|(i, (speaker, text))| {
            let t0 = i as i64 * 14_000;
            Segment {
                id: i as i64 + 1,
                meeting_id,
                track: if *speaker == 10 {
                    Track::Mic
                } else {
                    Track::System
                },
                t0_ms: t0,
                t1_ms: t0 + 11_000,
                text: (*text).into(),
                words: Vec::new(),
                speaker_id: Some(*speaker),
            }
        })
        .collect()
}

fn summary(meeting_id: i64) -> StoredSummary {
    StoredSummary {
        meeting_id,
        title: "Roadmap review".into(),
        short_summary: "The team reviewed the onboarding rework and meeting-notes quality. \
            Onboarding drop-off fell from 20% to 8% after merging the permission screens; \
            slow connections will get the smaller Parakeet build by default. Bob will draft \
            a voice-matching confidence threshold by Thursday."
            .into(),
        decisions: vec![
            "Default new users on slow connections to the smaller Parakeet build.".into(),
            "Show the model download estimate before the download starts.".into(),
            "Generate meeting titles from the decisions, not the first minute.".into(),
        ],
        topics: vec![
            "Onboarding".into(),
            "Model downloads".into(),
            "Meeting titles".into(),
            "Speaker naming".into(),
        ],
        generated_by: "anthropic/claude-sonnet-4-5".into(),
        generated_at: BASE_MS,
        usage: None,
        notes: vec![
            NoteSection {
                heading: "Onboarding rework".into(),
                start: "00:00".into(),
                start_ms: Some(0),
                points: vec![
                    "Microphone and accessibility now share one screen with a live check.".into(),
                    "Beta drop-off at the permissions step fell from ~20% to ~8%.".into(),
                ],
                outcome: Some("Ship behind a flag and collect feedback for a week.".into()),
            },
            NoteSection {
                heading: "Model download size".into(),
                start: "00:56".into(),
                start_ms: Some(56_000),
                points: vec![
                    "Users on slow connections struggled with the default download.".into(),
                    "Show the size and time estimate before committing.".into(),
                ],
                outcome: None,
            },
            NoteSection {
                heading: "Meeting notes quality".into(),
                start: "01:38".into(),
                start_ms: Some(98_000),
                points: vec![
                    "Titles are bland; derive them from the decisions.".into(),
                    "Speaker naming asks too often; remember confirmed voices.".into(),
                ],
                outcome: Some("Bob drafts a confidence threshold by Thursday.".into()),
            },
        ],
        open_questions: vec!["What confidence threshold is safe for auto-naming?".into()],
    }
}

fn people() -> Vec<Person> {
    vec![
        person_fixture(7, "Ada Lovelace", Some("ada@example.com")),
        person_fixture(8, "Bob Martin", Some("bob@example.com")),
        person_fixture(9, "Priya Shah", None),
        person_fixture(10, "Chen Wei", Some("chen@example.com")),
    ]
}

/// Settings and data for a populated, onboarded app.
fn populate(h: &Harness) {
    {
        let mut d = h.backend.data.lock();
        d.models = models();
        d.current_model = "parakeet-tdt-0.6b-v3".into();
        d.loaded_model = Some("parakeet-tdt-0.6b-v3".into());
        d.history = history();
        d.usage = usage();
        d.pp_models = vec!["claude-sonnet-4-5".into(), "gpt-4.1-mini".into()];
    }
    h.backend.with_settings(|s| {
        s.selected_model = "parakeet-tdt-0.6b-v3".into();
        s.meetings.enabled = true;
        s.post_process_enabled = true;
        s.debug_mode = true;
    });
    let mut d = h.meetings.data.lock();
    // Everything granted: no setup checklist above the list.
    d.permissions = guidy_core::meetings::manager::PermissionReport {
        calendar_granted: true,
        system_audio_granted: true,
        microphone_granted: true,
        notifications_granted: true,
    };
    d.meetings = vec![
        meeting(
            DETAIL_ID,
            "Roadmap review",
            60,
            32,
            MeetingStatus::Processed,
        ),
        meeting(
            2,
            "Design crit: recording widget",
            26 * 60,
            45,
            MeetingStatus::Processed,
        ),
        meeting(
            3,
            "1:1 with Priya",
            2 * 24 * 60,
            28,
            MeetingStatus::Processed,
        ),
        meeting(
            4,
            "Customer call: Northwind",
            3 * 24 * 60,
            52,
            MeetingStatus::Recorded,
        ),
        meeting(5, "Weekly sync", 7 * 24 * 60, 38, MeetingStatus::Processed),
        meeting(
            6,
            "Hiring loop debrief",
            9 * 24 * 60,
            21,
            MeetingStatus::Processed,
        ),
    ];
    d.people = people();
    d.upcoming = vec![guidy_core::meetings::types::CalendarEvent {
        id: "standup".into(),
        title: "Team standup".into(),
        start_at: BASE_MS + 30 * 60_000,
        end_at: BASE_MS + 45 * 60_000,
        meeting_url: Some("https://meet.google.com/abc".into()),
        attendees: vec![],
    }];
    for id in [DETAIL_ID, 2, 5] {
        d.speakers.insert(
            id,
            vec![
                speaker(10, id, "me", None, true),
                speaker(11, id, "Speaker 1", Some(7), false),
                speaker(12, id, "Speaker 2", Some(8), false),
            ],
        );
        d.segments.insert(id, transcript(id));
    }
    // Each person's voiceprints, as many as their count says: a line of
    // theirs from a meeting where one is theirs, else their average voice.
    let samples: Vec<(PersonId, Vec<guidy_core::meetings::types::VoiceSample>)> = d
        .people
        .iter()
        .map(|p| {
            let samples = [DETAIL_ID, 2, 5]
                .into_iter()
                .take(p.voiceprint_count as usize)
                .enumerate()
                .map(|(i, meeting_id)| {
                    let meeting = d.meetings.iter().find(|m| m.id == meeting_id);
                    let speaker = d
                        .speakers
                        .get(&meeting_id)
                        .and_then(|s| s.iter().find(|s| s.person_id == Some(p.id)))
                        .map(|s| s.id);
                    let segment = speaker.and_then(|sp| {
                        d.segments
                            .get(&meeting_id)
                            .and_then(|lines| {
                                // A different line of theirs per meeting.
                                lines.iter().filter(|l| l.speaker_id == Some(sp)).nth(i)
                            })
                            .cloned()
                    });
                    guidy_core::meetings::types::VoiceSample {
                        voiceprint_id: p.id * 10 + i as i64,
                        meeting_id: Some(meeting_id),
                        meeting_title: meeting.map(|m| m.title.clone()),
                        meeting_started_at: meeting.map(|m| m.started_at),
                        has_audio: segment.is_some(),
                        segment,
                    }
                })
                .collect();
            (p.id, samples)
        })
        .collect();
    d.voice_samples.extend(samples);
    for id in [DETAIL_ID, 2, 3, 5] {
        let mut s = summary(id);
        if id != DETAIL_ID {
            s.title = d
                .meetings
                .iter()
                .find(|m| m.id == id)
                .unwrap()
                .title
                .clone();
        }
        d.ai.summaries.insert(id, s);
    }
    let mut a = action_item_fixture(
        1,
        DETAIL_ID,
        "Draft the voice-matching confidence threshold",
    );
    a.owner_person_id = Some(8);
    a.owner_name = Some("Bob Martin".into());
    a.due = Some("Thursday".into());
    a.source_quote = "Yes, Thursday works. I'll send it to both of you for review.".into();
    a.source_t_ms = Some(13 * 14_000);
    let mut b = action_item_fixture(
        2,
        DETAIL_ID,
        "Default slow connections to the small Parakeet build",
    );
    b.owner_is_user = true;
    b.source_t_ms = Some(6 * 14_000);
    let c = action_item_fixture(
        3,
        DETAIL_ID,
        "Show the download estimate before the model download",
    );
    d.ai.items = vec![a, b, c];
}

// -- scenarios --------------------------------------------------------------------

type Build = fn(&mut Harness, Size<Pixels>) -> AnyWindowHandle;

struct Shot {
    name: String,
    size: Size<Pixels>,
    build: Build,
}

/// The Models page with the download manager busy: a speech model
/// downloading, an LLM queued, a speaker model failed, partial downloads to
/// resume, LLMs and speaker models on disk, and disk usage.
fn models_downloads(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    use guidy_core::downloads::{DiskUsage, DownloadItem, DownloadKind, DownloadState, OtherModel};
    populate(h);
    {
        let mut d = h.backend.data.lock();
        if let Some(large) = d.models.iter_mut().find(|m| m.id == "large") {
            large.partial_size = 310 * 1024 * 1024;
        }
        let mut turbo = DownloadItem::new(
            "stt:turbo",
            "Whisper Turbo",
            1549 * 1024 * 1024,
            DownloadState::Downloading,
        );
        turbo.downloaded_bytes = 620 * 1024 * 1024;
        turbo.bytes_per_sec = 11_200_000;
        let queued = DownloadItem::new(
            "llm:qwen3-8b-q4_k_m",
            "Qwen3 8B",
            5_027_783_488,
            DownloadState::Queued,
        );
        let mut failed = DownloadItem::new(
            "separation:titanet_large",
            "NVIDIA TitaNet Large",
            101_405_493,
            DownloadState::Failed,
        );
        failed.downloaded_bytes = 12_000_000;
        failed.error = Some("Connection reset by peer".into());
        d.downloads = vec![turbo, queued, failed];
        let other = |id: &str, kind, name: &str, size: u64, installed: bool| OtherModel {
            id: id.into(),
            kind,
            name: name.into(),
            description: String::new(),
            size_bytes: size,
            installed,
            partial_bytes: 0,
            is_default: false,
            recommended_min_ram_gb: None,
            fits_this_mac: true,
        };
        let mut qwen4 = other(
            "llm:qwen3-4b-instruct-2507-q4_k_m",
            DownloadKind::Llm,
            "Qwen3 4B Instruct",
            2_497_281_120,
            true,
        );
        qwen4.is_default = true;
        qwen4.description = "Fast, good summaries. Works on any Apple silicon Mac.".into();
        let mut qwen8 = other(
            "llm:qwen3-8b-q4_k_m",
            DownloadKind::Llm,
            "Qwen3 8B",
            5_027_783_488,
            false,
        );
        qwen8.description =
            "Better action items and owners. Recommended with 16 GB of memory or more.".into();
        qwen8.recommended_min_ram_gb = Some(16);
        let mut resnet293 = other(
            "separation:resnet293",
            DownloadKind::Separation,
            "WeSpeaker ResNet293",
            114_336_527,
            false,
        );
        resnet293.partial_bytes = 41_000_000;
        let mut resnet34 = other(
            "speaker:resnet34",
            DownloadKind::Speaker,
            "WeSpeaker ResNet34",
            26_530_550,
            true,
        );
        resnet34.is_default = true;
        d.other_models = vec![
            qwen4,
            qwen8,
            resnet34,
            other(
                "separation:cam_plus_plus",
                DownloadKind::Separation,
                "WeSpeaker CAM++",
                29_292_687,
                false,
            ),
            other(
                "separation:titanet_large",
                DownloadKind::Separation,
                "NVIDIA TitaNet Large",
                101_405_493,
                false,
            ),
            resnet293,
            other(
                "segmentation:pyannote",
                DownloadKind::Segmentation,
                "Pyannote Segmentation 3.0",
                5_992_913,
                true,
            ),
        ];
        d.disk_usage = DiskUsage {
            models_bytes: 4_120_000_000,
            free_bytes: Some(120_400_000_000),
        };
        d.ram_gb = Some(16);
        d.apple_intelligence = true;
        d.providers_with_keys = vec!["openai".into()];
    }
    h.install();
    let (w, main) = h.open_main(size);
    h.cx.update(|cx| main.update(cx, |m, cx| m.select_section(Section::Models, cx)));
    h.settle();
    w
}

fn main_at(h: &mut Harness, size: Size<Pixels>, section: Section) -> AnyWindowHandle {
    populate(h);
    h.install();
    let (w, main) = h.open_main(size);
    h.cx.update(|cx| main.update(cx, |m, cx| m.select_section(section, cx)));
    h.settle();
    w
}

macro_rules! section_build {
    ($section:expr) => {
        (|h, size| main_at(h, size, $section)) as Build
    };
}

fn meetings_tab(h: &mut Harness, size: Size<Pixels>, tab: &'static str) -> AnyWindowHandle {
    let w = main_at(h, size, Section::Meetings);
    h.with_window(w, |window, cx| window.click(tab, cx));
    w
}

fn meeting_detail(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    populate(h);
    h.install();
    let (w, main) = h.open_main(size);
    h.cx.update(|cx| {
        main.update(cx, |m, cx| {
            m.open_meeting_link(
                crate::logic::meetings::MeetingLink {
                    meeting_id: DETAIL_ID,
                    t_ms: None,
                },
                cx,
            )
        })
    });
    h.settle();
    w
}

/// `meeting_detail`, and the open meeting's view.
fn detail_and_view(
    h: &mut Harness,
    size: Size<Pixels>,
) -> (
    AnyWindowHandle,
    gpui_kit::Entity<crate::ui::pages::meetings::detail::MeetingDetail>,
) {
    populate(h);
    h.install();
    let (w, main) = h.open_main(size);
    h.cx.update(|cx| {
        main.update(cx, |m, cx| {
            m.open_meeting_link(
                crate::logic::meetings::MeetingLink {
                    meeting_id: DETAIL_ID,
                    t_ms: None,
                },
                cx,
            )
        })
    });
    h.settle();
    let page =
        h.cx.read_entity(&main, |m, _| m.meetings_page())
            .expect("the meetings page");
    let detail =
        h.cx.read_entity(&page, |p, _| p.detail().cloned())
            .expect("an open meeting");
    (w, detail)
}

/// A meeting being recorded, with its live transcript.
fn meeting_recording(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    use crate::ui::recording_widget::RecordingWidgetState;
    use guidy_core::meetings::types::{LiveLine, LiveStatus, LiveUpdate, RecorderState};
    populate(h);
    let recorder = RecorderState::Recording {
        meeting_id: 9,
        title: "Design review: recording widget".into(),
        // The timer reads the wall clock: about 12:34 in.
        started_at: guidy_core::meetings::recorder::now_ms() - 754_000,
        scheduled_end_at: None,
        paused_at: None,
        paused_ms: 0,
    };
    {
        let mut d = h.meetings.data.lock();
        let mut m = meeting(
            9,
            "Design review: recording widget",
            13,
            0,
            MeetingStatus::Recording,
        );
        m.ended_at = None;
        d.meetings.insert(0, m);
        d.recorder = recorder.clone();
    }
    h.install();
    let widget = h.cx.update(RecordingWidgetState::install);
    let lines = [
        (
            Track::System,
            "Okay, can everyone see my screen? This is the new floating recorder.",
            false,
        ),
        (
            Track::Mic,
            "Yes, looks good. Is the transcript live or does it wait for the end?",
            false,
        ),
        (
            Track::System,
            "It's live. Lines show up about a second after someone speaks",
            true,
        ),
    ];
    h.cx.update(|cx| {
        widget.update(cx, |s, cx| {
            s.animate = false;
            s.set_recorder(recorder, cx);
            for (i, (track, text, tentative)) in lines.iter().enumerate() {
                s.apply_live(
                    &LiveUpdate {
                        meeting_id: 9,
                        status: Some(LiveStatus::Live),
                        line: Some(LiveLine {
                            id: i as u64,
                            track: *track,
                            t0_ms: i as i64 * 3_500,
                            t1_ms: i as i64 * 3_500 + 3_000,
                            text: (*text).into(),
                            tentative: *tentative,
                        }),
                    },
                    cx,
                );
            }
        })
    });
    let (w, main) = h.open_main(size);
    h.cx.update(|cx| {
        main.update(cx, |m, cx| {
            m.open_meeting_link(
                crate::logic::meetings::MeetingLink {
                    meeting_id: 9,
                    t_ms: None,
                },
                cx,
            )
        })
    });
    h.settle();
    w
}

/// A meeting being transcribed.
fn meeting_processing(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    use guidy_core::meetings::types::{ProcessingProgress, ProcessingStage};
    populate(h);
    {
        let mut d = h.meetings.data.lock();
        if let Some(m) = d.meetings.iter_mut().find(|m| m.id == 4) {
            m.status = MeetingStatus::Processing;
        }
        d.progress.insert(
            4,
            ProcessingProgress {
                meeting_id: 4,
                stage: ProcessingStage::Transcribe,
                fraction: 0.62,
                message: None,
                done: None,
                total: None,
            },
        );
    }
    h.install();
    let (w, main) = h.open_main(size);
    h.cx.update(|cx| {
        main.update(cx, |m, cx| {
            m.open_meeting_link(
                crate::logic::meetings::MeetingLink {
                    meeting_id: 4,
                    t_ms: None,
                },
                cx,
            )
        })
    });
    h.settle();
    w
}

/// Find in the transcript, three matches.
fn meeting_find(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    let (w, detail) = detail_and_view(h, size);
    h.cx.update_window(w, |_, window, cx| {
        detail.update(cx, |d, cx| {
            d.open_find(window, cx);
            d.type_find("the", window, cx);
        })
    })
    .unwrap();
    h.settle();
    w
}

/// The player mid-meeting, its line marked.
fn meeting_playing(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    let (w, detail) = detail_and_view(h, size);
    let player = h.cx.read_entity(&detail, |d, _| d.player().clone());
    h.cx.update(|cx| player.update(cx, |p, cx| p.preview(5 * 14_000 + 3_000, true, cx)));
    h.with_window(w, |window, cx| {
        window.click("meeting-detail-tab-transcript", cx)
    });
    w
}

/// No meetings yet.
fn meetings_empty(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    populate(h);
    h.meetings.data.lock().meetings.clear();
    h.install();
    let (w, main) = h.open_main(size);
    h.cx.update(|cx| main.update(cx, |m, cx| m.select_section(Section::Meetings, cx)));
    h.settle();
    w
}

fn meeting_transcript(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    let w = meeting_detail(h, size);
    h.with_window(w, |window, cx| {
        window.click("meeting-detail-tab-transcript", cx)
    });
    w
}

/// Onboarding fixtures: no speech model downloaded, the meeting models and
/// LLMs listed (none on disk), room on disk, Apple Intelligence available.
fn onboarding_fixture(h: &Harness) {
    use guidy_core::downloads::{DiskUsage, DownloadKind, OtherModel};
    let other = |id: &str, kind, name: &str, size: u64, default: bool| OtherModel {
        id: id.into(),
        kind,
        name: name.into(),
        description: String::new(),
        size_bytes: size,
        installed: false,
        partial_bytes: 0,
        is_default: default,
        recommended_min_ram_gb: None,
        fits_this_mac: true,
    };
    let mut d = h.backend.data.lock();
    d.models = models()
        .into_iter()
        .map(|mut m| {
            m.is_downloaded = false;
            m.is_downloading = false;
            m.partial_size = 0;
            m
        })
        .collect();
    let mut qwen8 = other(
        "llm:qwen3-8b-q4_k_m",
        DownloadKind::Llm,
        "Qwen3 8B",
        5_027_783_488,
        false,
    );
    qwen8.recommended_min_ram_gb = Some(16);
    d.other_models = vec![
        other(
            "llm:qwen3-4b-instruct-2507-q4_k_m",
            DownloadKind::Llm,
            "Qwen3 4B Instruct",
            2_497_281_120,
            true,
        ),
        qwen8,
        other(
            "speaker:resnet34",
            DownloadKind::Speaker,
            "WeSpeaker ResNet34",
            26_530_550,
            true,
        ),
        other(
            "separation:titanet_large",
            DownloadKind::Separation,
            "NVIDIA TitaNet Large",
            101_405_493,
            false,
        ),
        other(
            "segmentation:pyannote",
            DownloadKind::Segmentation,
            "Pyannote Segmentation 3.0",
            5_992_913,
            false,
        ),
    ];
    d.disk_usage = DiskUsage {
        models_bytes: 0,
        free_bytes: Some(120_400_000_000),
    };
    d.ram_gb = Some(16);
    d.apple_intelligence = true;
}

/// The main window at an onboarding `step`, with `choices` made.
fn onboarding_at(
    h: &mut Harness,
    size: Size<Pixels>,
    step: OnboardingStep,
    choices: crate::logic::onboarding::Choices,
) -> AnyWindowHandle {
    onboarding_fixture(h);
    if step == OnboardingStep::Migration {
        {
            let mut d = h.backend.data.lock();
            d.retired = vec![RetiredModel {
                id: "parakeet-tdt-0.6b-v2".into(),
                name: "Parakeet V2".into(),
                size_mb: 473,
                // Parakeet V3's repo (see `model`), so it pairs up.
                replacement_repo: "handy-computer/parakeet-tdt-0.6b-v3-gguf".into(),
            }];
        }
        h.backend
            .with_settings(|s| s.selected_model = "parakeet-tdt-0.6b-v2".into());
    } else {
        h.backend.with_settings(|s| s.onboarding_completed = false);
        let mut d = h.backend.data.lock();
        d.accessibility = false;
        d.microphone = true;
    }
    h.install();
    let (w, main) = h.open_main(size);
    if !matches!(step, OnboardingStep::Welcome | OnboardingStep::Migration) {
        h.cx.update(|cx| {
            AppState::global(cx).update(cx, |s, cx| {
                s.startup.choices = choices;
                s.set_step(step, cx);
            })
        });
        h.settle();
    }
    let got = h.cx.read_entity(&main, |m, cx| m.step(cx));
    assert_eq!(got, Some(step), "onboarding did not reach {step:?}");
    w
}

fn onboarding(h: &mut Harness, size: Size<Pixels>, step: OnboardingStep) -> AnyWindowHandle {
    onboarding_at(h, size, step, Default::default())
}

/// The setup step for dictation + meeting notes, summaries `where`.
fn onboarding_meetings(
    h: &mut Harness,
    size: Size<Pixels>,
    summaries: Option<crate::logic::onboarding::Summaries>,
) -> AnyWindowHandle {
    let choices = crate::logic::onboarding::Choices {
        meetings: true,
        summaries,
        ..Default::default()
    };
    onboarding_at(h, size, OnboardingStep::Setup, choices)
}

/// "Choose Another…" opened from the setup step.
fn onboarding_picker(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    let w = onboarding(h, size, OnboardingStep::Setup);
    h.with_window(w, |window, cx| window.click("setup-choose-speech", cx));
    w
}

/// The window while the core starts (`expect_app`: a returning user), or
/// after it failed (`error`).
fn startup(
    h: &mut Harness,
    size: Size<Pixels>,
    expect_app: bool,
    error: Option<&str>,
) -> AnyWindowHandle {
    let backend = h.backend.clone();
    let meetings = h.meetings.clone();
    let theme = h.theme;
    let error = error.map(str::to_string);
    h.cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::ui::init(cx);
        guidy_i18n::set_language("en");
        let state = AppState::install_starting(backend, meetings, expect_app, cx);
        if let Some(e) = error {
            state.update(cx, |s, cx| s.core_failed(e, cx));
        }
        crate::ui::overlay::OverlayState::install(cx);
        crate::theme::apply_setting(theme, cx);
    });
    h.settle();
    let (w, _) = h.open_main(size);
    w
}

fn overlay(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
    h.backend
        .with_settings(|s| s.overlay_style = guidy_core::settings::OverlayStyle::Live);
    h.wall_wait = Duration::from_millis(800);
    h.install();
    let (w, _) = h.open(size, false, crate::ui::overlay::OverlayView::new);
    let state = h.cx.update(|cx| AppState::global(cx));
    let event = |name: &str, payload: serde_json::Value| guidy_core::host::Event {
        name: name.into(),
        target: None,
        payload,
    };
    let events = [
        event("show-overlay", serde_json::json!("streaming")),
        event("recording-ready", serde_json::Value::Null),
        event(
            "mic-level",
            serde_json::json!([
                0.2, 0.5, 0.8, 0.6, 0.9, 0.4, 0.7, 0.3, 0.5, 0.6, 0.2, 0.8, 0.4, 0.6, 0.3, 0.5
            ]),
        ),
        event(
            "stream-text-event",
            serde_json::json!({"committed": "Guidy turns your voice into text, right where", "tentative": "your cursor is"}),
        ),
    ];
    for ev in events {
        h.cx.update(|cx| {
            let overlay = crate::ui::overlay::OverlayState::global(cx).unwrap();
            crate::dispatch_event(&ev, &state, &overlay, cx);
        });
        h.settle();
    }
    w
}

fn recording_widget(h: &mut Harness, size: Size<Pixels>, expanded: bool) -> AnyWindowHandle {
    use crate::ui::recording_widget::{RecordingWidgetState, RecordingWidgetView};
    use guidy_core::meetings::types::{LiveLine, LiveStatus, LiveUpdate, RecorderState};
    h.wall_wait = Duration::from_millis(800);
    h.install();
    let widget = h.cx.update(RecordingWidgetState::install);
    let (width, height) = (f32::from(size.width) as f64, f32::from(size.height) as f64);
    h.cx.update(|cx| {
        widget.update(cx, |s, _| {
            s.animate = false;
            s.docked = false;
            s.desk =
                crate::logic::floater::Desk::single(crate::logic::floater::Stage::from_screen(
                    &crate::logic::notch::ScreenInfo {
                        x: 0.0,
                        y: 0.0,
                        width,
                        height,
                        notch: None,
                        top_inset: 0.0,
                    },
                    crate::ui::design::HUD_SCALE,
                ));
        })
    });
    let (w, _) = h.open(size, false, |window, cx| {
        RecordingWidgetView::new(0, window, cx)
    });
    let lines = [
        (
            Track::System,
            "Okay, can everyone see my screen? This is the new floating recorder.",
            false,
        ),
        (
            Track::Mic,
            "Yes, looks good. Is the transcript live or does it wait for the end?",
            false,
        ),
        (
            Track::System,
            "It's live. Lines show up about a second after someone speaks",
            true,
        ),
    ];
    h.cx.update(|cx| {
        widget.update(cx, |s, cx| {
            s.set_recorder(
                RecorderState::Recording {
                    meeting_id: 9,
                    title: "Design review: recording widget".into(),
                    // The timer reads the wall clock: about 12:34 in.
                    started_at: guidy_core::meetings::recorder::now_ms() - 754_000,
                    scheduled_end_at: None,
                    paused_at: None,
                    paused_ms: 0,
                },
                cx,
            );
            for (i, (track, text, tentative)) in lines.iter().enumerate() {
                s.apply_live(
                    &LiveUpdate {
                        meeting_id: 9,
                        status: Some(LiveStatus::Live),
                        line: Some(LiveLine {
                            id: i as u64,
                            track: *track,
                            t0_ms: i as i64 * 3_500,
                            t1_ms: i as i64 * 3_500 + 3_000,
                            text: (*text).into(),
                            tentative: *tentative,
                        }),
                    },
                    cx,
                );
            }
        })
    });
    h.settle();
    if expanded {
        h.cx.update(|cx| crate::ui::recording_widget::set_expanded(true, cx));
        h.settle();
    }
    w
}

// ---- meeting settings panes and People (settings agent) -------------------------
//
// The views render in a stand-in for the main window's frame (sidebar,
// title bar, status bar) so they get the real content area and window width,
// whatever the Meetings page around them does at the moment.

mod settings_panes {
    use super::*;
    use crate::ui::design;
    use crate::ui::pages::meetings::people::PeopleView;
    use crate::ui::pages::meetings::settings::{MeetingSettingsView, SettingsCategory};
    use gpui_kit::component::{h_flex, v_flex};
    use gpui_kit::prelude::*;
    use gpui_kit::{div, Context, Entity, Render, Window};

    struct Frame<V: 'static> {
        view: Entity<V>,
    }

    impl<V: Render> Render for Frame<V> {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let tone = design::tone(cx);
            h_flex()
                .size_full()
                .items_stretch()
                .bg(tone.frame)
                .child(div().flex_none().w(design::SIDEBAR_W).h_full())
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .bg(tone.ink)
                        .child(
                            div()
                                .flex_none()
                                .h(design::TOOLBAR_H)
                                .border_b_1()
                                .border_color(tone.hairline),
                        )
                        .child(div().flex_1().min_h_0().child(self.view.clone()))
                        .child(
                            div()
                                .flex_none()
                                .h(design::STATUS_BAR_H)
                                .border_t_1()
                                .border_color(tone.hairline)
                                .bg(tone.frame),
                        ),
                )
        }
    }

    fn cloud_models(h: &Harness) {
        use guidy_core::llm::registry::{Cost, Limits, ModelEntry, RegistryModel};
        let entry = |id: &str, name: &str, ctx: u64, i: f64, o: f64, reasoning: bool| ModelEntry {
            id: id.into(),
            name: name.into(),
            info: Some(RegistryModel {
                id: id.into(),
                name: name.into(),
                structured_output: true,
                reasoning,
                limit: Limits {
                    context: Some(ctx),
                    output: None,
                },
                cost: Some(Cost {
                    input: Some(i),
                    output: Some(o),
                }),
                ..Default::default()
            }),
        };
        let mut models = vec![
            entry(
                "claude-opus-4-1",
                "Claude Opus 4.1",
                200_000,
                15.0,
                75.0,
                true,
            ),
            entry(
                "claude-sonnet-4-5",
                "Claude Sonnet 4.5",
                200_000,
                3.0,
                15.0,
                true,
            ),
            entry(
                "claude-haiku-4-5",
                "Claude Haiku 4.5",
                200_000,
                1.0,
                5.0,
                false,
            ),
            entry(
                "claude-3-7-sonnet",
                "Claude Sonnet 3.7",
                200_000,
                3.0,
                15.0,
                true,
            ),
            entry(
                "claude-3-5-haiku",
                "Claude Haiku 3.5",
                200_000,
                0.8,
                4.0,
                false,
            ),
        ];
        for i in 0..14 {
            models.push(entry(
                &format!("claude-preview-{i}"),
                &format!("Claude Preview {i}"),
                100_000,
                2.0,
                8.0,
                false,
            ));
        }
        h.meetings.data.lock().ai.catalog = crate::meetings_backend::ModelCatalog {
            models,
            in_registry: true,
            registry_fetched_at: None,
            live_error: None,
        };
        h.backend.with_settings(|s| {
            s.meetings.llm_provider_id = "anthropic".into();
            s.meetings
                .llm_models
                .insert("anthropic".into(), "claude-sonnet-4-5".into());
            s.meetings.llm_saved_models = vec![guidy_core::meetings::settings::SavedLlmModel {
                provider_id: "openai".into(),
                model: "gpt-4.1".into(),
            }];
        });
    }

    fn pane(h: &mut Harness, size: Size<Pixels>, category: SettingsCategory) -> AnyWindowHandle {
        populate(h);
        if category == SettingsCategory::Ai {
            cloud_models(h);
        }
        if category == SettingsCategory::Setup {
            // Something to set up: no system audio, no speaker model.
            let mut d = h.meetings.data.lock();
            d.permissions.system_audio_granted = false;
            d.permissions.calendar_granted = false;
            d.speaker_model = guidy_core::meetings::processing::SpeakerModelStatus::Missing;
        }
        h.install();
        let (w, frame) = h.open(size, true, |window, cx| Frame {
            view: cx.new(|cx| MeetingSettingsView::new(window, cx)),
        });
        let view = h.cx.update(|cx| frame.read(cx).view.clone());
        h.cx.update(|cx| view.update(cx, |v, cx| v.show_category(category, cx)));
        h.settle();
        w
    }

    fn people(h: &mut Harness, size: Size<Pixels>, person: Option<i64>) -> AnyWindowHandle {
        populate(h);
        h.install();
        let (w, frame) = h.open(size, true, |window, cx| Frame {
            view: cx.new(|cx| PeopleView::new(window, cx)),
        });
        if let Some(id) = person {
            let view = h.cx.update(|cx| frame.read(cx).view.clone());
            h.cx.update_window(w, |_, window, cx| {
                view.update(cx, |v, cx| v.select_person(id, window, cx))
            })
            .unwrap();
            h.settle();
        }
        w
    }

    macro_rules! pane_build {
        ($c:expr) => {
            (|h, size| pane(h, size, $c)) as Build
        };
    }

    pub(super) fn shots(add: &mut dyn FnMut(&str, Size<Pixels>, Build)) {
        let all: [(&str, Build); 9] = [
            ("setup", pane_build!(SettingsCategory::Setup)),
            ("general", pane_build!(SettingsCategory::General)),
            ("you", pane_build!(SettingsCategory::You)),
            ("recording", pane_build!(SettingsCategory::Recording)),
            (
                "transcription",
                pane_build!(SettingsCategory::Transcription),
            ),
            ("speakers", pane_build!(SettingsCategory::Speakers)),
            ("ai", pane_build!(SettingsCategory::Ai)),
            ("linear", pane_build!(SettingsCategory::Linear)),
            ("integrations", pane_build!(SettingsCategory::Integrations)),
        ];
        for (id, build) in all {
            add(&format!("msettings-{id}"), default_size(), build);
        }
        add(
            "msettings-ai-min",
            min_size(),
            pane_build!(SettingsCategory::Ai),
        );
        add(
            "msettings-ai-wide",
            wide_size(),
            pane_build!(SettingsCategory::Ai),
        );
        add(
            "msettings-setup-min",
            min_size(),
            pane_build!(SettingsCategory::Setup),
        );
        add("mpeople-you", default_size(), |h, s| people(h, s, None));
        add("mpeople-person", default_size(), |h, s| {
            people(h, s, Some(8))
        });
        add("mpeople-person-min", min_size(), |h, s| {
            people(h, s, Some(8))
        });
    }
}
// ---- end meeting settings panes and People ---------------------------------------

// ---- settings pages redesign: History, Advanced, Post Process, Usage states ----
mod settings_pages {
    use super::*;

    fn type_enter(h: &mut Harness, w: AnyWindowHandle, id: &'static str, text: &'static str) {
        h.with_window(w, |window, cx| {
            window.click(id, cx);
            window.press("cmd-a", cx);
            window.input(text, cx);
            window.press("enter", cx);
        });
    }

    /// History with the second transcription selected.
    fn history_selected(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
        let w = main_at(h, size, Section::History);
        h.with_window(w, |window, cx| window.click("history-entry-99", cx));
        w
    }

    /// Right after a delete: the row is gone and Undo is offered.
    fn history_undo(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
        let w = main_at(h, size, Section::History);
        h.with_window(w, |window, cx| window.click("delete-100", cx));
        w
    }

    /// Lowering the history limit asks before deleting.
    fn limit_confirm(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
        h.backend.with_settings(|s| {
            s.history_limit = 50;
            s.recording_retention_period =
                guidy_core::settings::RecordingRetentionPeriod::PreserveLimit;
        });
        let w = main_at(h, size, Section::Advanced);
        type_enter(h, w, "history-limit-input", "2");
        w
    }

    /// Switching prompts with unsaved edits asks first.
    fn prompt_discard(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
        h.backend.with_settings(|s| {
            let mut second = s.post_process_prompts[0].clone();
            second.id = "second".into();
            second.name = "Meeting notes".into();
            s.post_process_prompts.push(second);
            s.post_process_selected_prompt_id = Some(s.post_process_prompts[0].id.clone());
        });
        let w = main_at(h, size, Section::PostProcessing);
        h.with_window(w, |window, cx| {
            window.click("pp-prompt-text", cx);
            window.press("cmd-a", cx);
            window.input("Rewrite as a short bulleted list: ${output}", cx);
        });
        h.with_window(w, |window, cx| window.click("pp-prompt-second", cx));
        w
    }

    /// The usage report could not be read.
    fn usage_error(h: &mut Harness, size: Size<Pixels>) -> AnyWindowHandle {
        populate(h);
        h.backend.data.lock().fail_settings = vec!["llm_usage_report".into()];
        h.install();
        let (w, main) = h.open_main(size);
        h.cx.update(|cx| main.update(cx, |m, cx| m.select_section(Section::Usage, cx)));
        h.settle();
        w
    }

    pub(super) fn shots(add: &mut dyn FnMut(&str, Size<Pixels>, Build)) {
        add("history-selected", default_size(), history_selected);
        add("history-selected-min", min_size(), history_selected);
        add("history-undo", default_size(), history_undo);
        // Tall: the History group is below the fold at 600 pt.
        add("advanced-limit-confirm", tall_size(), limit_confirm);
        add("postprocessing-discard", default_size(), prompt_discard);
        add("usage-error", default_size(), usage_error);
    }
}
// ---- end settings pages redesign -------------------------------------------------

fn shots() -> Vec<Shot> {
    let mut v = Vec::new();
    let mut add = |name: &str, size: Size<Pixels>, build: Build| {
        v.push(Shot {
            name: name.into(),
            size,
            build,
        })
    };
    let sections: [(&str, Build); 9] = [
        ("general", section_build!(Section::General)),
        ("history", section_build!(Section::History)),
        ("meetings", section_build!(Section::Meetings)),
        ("models", models_downloads as Build),
        ("advanced", section_build!(Section::Advanced)),
        ("postprocessing", section_build!(Section::PostProcessing)),
        ("usage", section_build!(Section::Usage)),
        ("debug", section_build!(Section::Debug)),
        ("about", section_build!(Section::About)),
    ];
    for (id, build) in sections {
        add(&format!("main-{id}"), default_size(), build);
        add(&format!("main-{id}-full"), tall_size(), build);
        add(&format!("main-{id}-min"), min_size(), build);
    }
    add(
        "main-meetings-wide",
        wide_size(),
        section_build!(Section::Meetings),
    );
    add("main-models-wide", wide_size(), models_downloads as Build);

    let people: Build = |h, s| meetings_tab(h, s, "meetings-tab-people");
    let settings: Build = |h, s| meetings_tab(h, s, "meetings-tab-settings");
    add("meetings-people", default_size(), people);
    add("meetings-people-full", tall_size(), people);
    add("meetings-settings", default_size(), settings);
    add("meetings-settings-full", tall_size(), settings);
    // Meeting settings panes and People (settings agent).
    settings_panes::shots(&mut add);
    // Settings pages redesign states.
    settings_pages::shots(&mut add);
    add("meetings-detail", default_size(), meeting_detail);
    add("meetings-detail-full", tall_size(), meeting_detail);
    add("meetings-detail-wide", wide_size(), meeting_detail);
    add("meetings-transcript", default_size(), meeting_transcript);
    add("meetings-transcript-full", tall_size(), meeting_transcript);
    add("meetings-detail-min", min_size(), meeting_detail);
    add("meetings-recording", default_size(), meeting_recording);
    add("meetings-recording-wide", wide_size(), meeting_recording);
    add("meetings-processing", default_size(), meeting_processing);
    add("meetings-find", default_size(), meeting_find);
    add("meetings-find-wide", wide_size(), meeting_find);
    add("meetings-playing", default_size(), meeting_playing);
    add("meetings-empty", default_size(), meetings_empty);

    // Onboarding and startup, at the default and the minimum size.
    {
        use crate::logic::onboarding::{Choices, Summaries};
        let onboarding_shots: [(&str, Build); 12] = [
            ("onboarding-welcome", |h, s| {
                onboarding(h, s, OnboardingStep::Welcome)
            }),
            ("onboarding-permissions", |h, s| {
                let choices = Choices {
                    meetings: true,
                    ..Default::default()
                };
                onboarding_at(h, s, OnboardingStep::Permissions, choices)
            }),
            ("onboarding-setup", |h, s| {
                onboarding(h, s, OnboardingStep::Setup)
            }),
            ("onboarding-setup-meetings", |h, s| {
                onboarding_meetings(h, s, None)
            }),
            ("onboarding-setup-apple", |h, s| {
                onboarding_meetings(h, s, Some(Summaries::AppleIntelligence))
            }),
            ("onboarding-setup-cloud", |h, s| {
                onboarding_meetings(h, s, Some(Summaries::Cloud("anthropic".into())))
            }),
            ("onboarding-setup-meetings-only", |h, s| {
                let choices = Choices {
                    dictation: false,
                    meetings: true,
                    ..Default::default()
                };
                onboarding_at(h, s, OnboardingStep::Setup, choices)
            }),
            ("onboarding-picker", onboarding_picker),
            ("onboarding-migration", |h, s| {
                onboarding(h, s, OnboardingStep::Migration)
            }),
            ("startup-app", |h, s| startup(h, s, true, None)),
            ("startup-new", |h, s| startup(h, s, false, None)),
            ("startup-error", |h, s| {
                startup(
                    h,
                    s,
                    true,
                    Some(
                        "Failed to initialize history manager: Permission denied (os error 13) \
                         at ~/Library/Application Support/com.guidy.app/history.db",
                    ),
                )
            }),
        ];
        for (name, build) in onboarding_shots {
            add(name, default_size(), build);
            add(&format!("{name}-min"), min_size(), build);
        }
    }

    add("overlay-live", size(px(520.), px(200.)), overlay);
    add("recording-widget", size(px(900.), px(640.)), |h, s| {
        recording_widget(h, s, false)
    });
    add(
        "recording-widget-expanded",
        size(px(900.), px(640.)),
        |h, s| recording_widget(h, s, true),
    );
    v
}

// -- the runner -------------------------------------------------------------------

fn out_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("GUIDY_SNAPSHOT_DIR") {
        return PathBuf::from(dir);
    }
    // crates/guidy-app -> workspace target/
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots")
}

fn wanted(name: &str) -> bool {
    match std::env::var("GUIDY_SNAPSHOT_FILTER") {
        Ok(f) if !f.trim().is_empty() => f
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .any(|p| name.contains(p)),
        _ => true,
    }
}

fn themes() -> Vec<(ThemeSetting, &'static str)> {
    let all = [(ThemeSetting::Light, "light"), (ThemeSetting::Dark, "dark")];
    match std::env::var("GUIDY_SNAPSHOT_THEME").as_deref() {
        Ok("light") => vec![all[0]],
        Ok("dark") => vec![all[1]],
        _ => all.to_vec(),
    }
}

#[test]
#[ignore = "writes PNGs; run with --ignored (docs/UI_SNAPSHOTS.md)"]
fn ui_snapshots() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("create snapshot dir");
    let dir = dir.canonicalize().unwrap_or(dir);
    let mut written = 0;
    for shot in shots().into_iter().filter(|s| wanted(&s.name)) {
        for (theme, label) in themes() {
            let mut h = Harness::new(theme);
            let w = (shot.build)(&mut h, shot.size);
            let path = dir.join(format!("{}-{label}.png", shot.name));
            let (width, height) = h.capture(w, &path);
            println!("ui-snapshot {} ({width}x{height})", path.display());
            written += 1;
        }
    }
    assert!(written > 0, "GUIDY_SNAPSHOT_FILTER matched no snapshot");
}
