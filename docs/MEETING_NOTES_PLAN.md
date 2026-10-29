# Guidy: meeting notes

Guidy records a meeting on the user's own Mac, transcribes it with named speakers, summarizes it, extracts action items, and drafts Linear tickets that the user approves before they are created. All of it can run on local models. Cloud models are available through the user's own API keys.

This plan was agreed on through a design review. The decisions behind it are recorded in §2, and each one marked **(default)** was chosen without a specific user answer and can be revisited.

---

## 1. Scope

### 1.1 In v1

- **macOS 14.2+ only** for meeting features. The rest of Guidy keeps its current minimum (10.15), and meeting features are hidden on older systems.
- Any meeting app works, because Guidy captures audio locally and no bot joins the call. Google Meet is the first app whose calendar events we recognize.
- Processing happens **after the meeting**, not live.
- Speakers get **real names**: the user names them after the meeting, and Guidy recognizes their voices in later meetings.
- Summaries and action items come from local models: built-in llama.cpp or Apple Intelligence.
- Approved action items become Linear tickets.
- The user can bring their own API keys for cloud providers.
- Every secret is stored in the macOS Keychain.

### 1.2 Roadmap (not in v1)

- A live transcript during the meeting
- Google Calendar sign-in, for users who don't use macOS Calendar
- Windows support (WASAPI loopback capture), then Linux (PipeWire)
- Linear sign-in (OAuth) instead of a personal API key
- Other trackers: Jira, GitHub Issues
- Ollama / LM Studio as first-class local options (they keep working through the existing Custom provider)

### 1.3 Non-goals

- Guidy does not announce recordings or ask participants for consent. That is the user's responsibility.
- Recording never starts automatically. Guidy asks first.
- Recording never stops automatically. Guidy only reminds the user (§3.1).

---

## 2. Decisions

| # | Topic | Decision |
|---|---|---|
| D1 | Audio source | Record on the user's own Mac: the microphone plus the computer's audio. No bot joins the call, and no browser extension. |
| D2 | Speaker identity | Real names. The user names speakers after the meeting, and Guidy remembers their voices for future meetings. |
| D3 | Timing | Process after the meeting. A live transcript is on the roadmap. |
| D4 | Platform | macOS first. |
| D5 | Start | Calendar-driven, with a manual start as fallback. The tray always has a start/stop item. |
| D6 | Consent | No disclosure features. Consent is the user's responsibility. |
| D7 | Task split | Four tasks (§3). The AI-model layer is built in task 2, not task 4. |
| D8 | System-audio capture | Core Audio process taps (macOS 14.2+). Capture all system audio except Guidy's own sounds. |
| D9 | Speakers without headphones | Echo cancellation of the mic in software: WebRTC's AEC3 (`sonora`, pure Rust) with the system-audio track as the reference. The mic itself is recorded plain. (Apple's Voice Processing I/O was used first and dropped: it failed to start on common setups, took over the output device, ducked other apps and could cut off a call app's input.) When attributing speech to the user, also compare the words and timing of the two tracks as a safety net, in the live transcript too. |
| D10 | Calendar source | macOS Calendar (EventKit). Google Calendar sign-in is on the roadmap. |
| D11 | Naming and voiceprints | Store voiceprints only for people the user names. Group speakers by clustering voiceprints, which has no speaker limit. A People list allows rename, merge and delete. |
| D12 | Where meetings appear | A new **Meetings** page, with **People** as a tab inside it. Dictation history is unchanged. |
| D13 | Raw audio retention | 3 days by default. The user can choose 3, 7 or 30 days, or forever. Deleting audio never deletes voiceprints. |
| D14 | Secrets | Every secret lives in the macOS Keychain, starting in task 1. |
| D15 | Local models | Built-in llama.cpp (default) and Apple Intelligence, both offered to the user. |
| D16 | Stop | Manual stop only, with recurring "still recording" reminders after the meeting should have ended (§3.1). |
| D17 | Sensitive data at rest | **(default)** The meetings database and the meeting audio are encrypted with a key held in the Keychain. Dictation history (`history.db`) is unchanged. |

---

## 3. Tasks

The tasks ship in order. Each one ends in a working, tested feature.

### Task 1: meeting capture, transcript and speakers

**1.0 Spike: voiceprint accuracy (about 2 days, before anything else)**
- Record real Google Meet calls, with 2, 4 and 6+ speakers, both with headphones and through speakers.
- Measure:
  - how accurately sherpa-onnx's voiceprints and clustering group speakers on Meet-compressed audio
  - how accurately people are matched across meetings
  - whether Apple's echo cancellation lowers other apps' volume (audio ducking)
- Output: a go/no-go decision on sherpa-onnx, the matching threshold, and whether Sortformer (from `transcribe-cpp`) is still needed for small meetings.

**1.1 Secrets module (Keychain)**
- A `secrets` module in `guidy-core` built on the `keyring` crate (Keychain on macOS), with a get/set/delete-by-key API.
- Also generate the encryption key for meeting data and keep it in the Keychain (D17).

**1.2 Calendar (EventKit)**
- Read events through a Swift bridge, following the pattern of `swift/apple_intelligence.swift`.
- Request calendar access (`NSCalendarsFullAccessUsageDescription`) the first time the user turns on meeting notes.
- Detect events that contain a Google Meet, Zoom or Teams link. Read the title, start and end times, the link, and the attendees' names and emails.
- At an event's start time, send a notification: "Take notes for *Weekly sync*? · Start". **(default)**

**1.3 Capture**
- Two tracks, written to disk as recording happens so that a crash doesn't lose the meeting:
  - **Mic**, plain input, echo-cancelled against the system track (D9). This track is always the user. **(default)** It is labelled with the user's name from settings.
  - **System audio**, through a Core Audio process tap that excludes Guidy's own process (`NSAudioCaptureUsageDescription`).
- Store audio as Opus, encrypted (D17). Delete it on the retention schedule (D13).
- Tray: "Start meeting notes" / "Stop meeting notes", the linked event's title, and a recording-state icon.
- A manual start links to whichever calendar event is happening at that moment, if there is one.
- Reminders (D16):
  - **Calendar meeting:** the first reminder comes 5 minutes after the scheduled end, then every 10 minutes: "Guidy is still recording · Stop / Keep going".
  - **Manual session** (no end time): reminders start after 60 minutes, then come every 10 minutes.

**1.4 Processing**
1. VAD splits each track into speech segments (the existing `audio_toolkit/vad`).
2. A batch model transcribes the segments with word timestamps. This uses a new "meeting model" setting, defaulting to Parakeet v3. **(default)** `transcribe-cpp` already returns `words`/`segments`; Guidy currently uses only `.text`.
3. On the system-audio track: compute a voiceprint per segment, cluster the voiceprints into speakers, and match each cluster against known people above a confidence threshold.
4. Merge both tracks into one timeline. Where the two tracks overlap in time and their text matches, attribute the speech to the remote speaker rather than the user (the safety net from D9).
5. Run the processing in the worker process, with progress shown in the UI. It can be cancelled and resumed.

**1.5 Data**
- A new encrypted SQLite database (SQLCipher) at `<app_data_dir>/meetings.db`:
  - `meetings`: id, title, calendar event id, started/ended, audio paths, status
  - `meeting_segments`: meeting, track (mic/system), t0/t1, text, word timestamps, speaker id
  - `meeting_speakers`: meeting, cluster label, person id, match confidence, confirmed
  - `people`: id, display name, email, created
  - `voiceprints`: person, embedding, source meeting, created
- Migrations go through `rusqlite_migration`, as in `history.rs`.

**1.6 User interface**
- Add **Meetings** to the sidebar, in the `Section` enum, `Pages` and `page!` in `main_window.rs`.
- Meeting list: title, date, duration, processing status.
- Meeting detail:
  - The transcript, grouped by speaker, with segment playback.
  - A **Speaker N → name** picker that suggests the event's attendees first, then known people, then free text.
  - Uncertain automatic matches carry a "Confirm?" indicator.
  - Naming a speaker stores that speaker's voiceprints under the person.
- **People** tab: rename, merge, delete. Deleting a person also deletes their voiceprints.
- Settings: meeting model, audio retention (3/7/30 days or forever), calendar notifications on/off, and the meeting-reminder interval.

**Done when:** a recorded Meet call with 3 named participants produces a transcript in which every speaker's name is correct after the first meeting's naming, and the next meeting with the same people is labelled automatically.

### Task 2: AI-model layer, summaries and action items

**2.1 Provider layer**
- One `LlmProvider` interface over chat completion and structured JSON output, generalizing `llm_client.rs`.
- Existing providers keep working, including Custom (OpenAI-compatible) and Apple Intelligence.
- Model choice is **per feature**: dictation post-processing and meetings can use different models. **(default)**

**2.2 Built-in llama.cpp**
- Runs in its own worker mode, alongside `--transcribe-worker`. A separate process avoids clashing with the copy of ggml that `transcribe-cpp` already bundles, and keeps Metal GPU acceleration.
- A GGUF catalog for language models, using the existing download, checksum and storage code. The default is a ~4B instruction-tuned model (e.g. Qwen3 4B), with an 8B option recommended for machines with 16 GB+ RAM. **(default)**

**2.3 Summary and action items**
- Long transcripts are summarized in chunks, and the chunk summaries are then combined. This covers both long meetings and Apple Intelligence's ~4k-token context.
- The output follows a fixed JSON structure **(default)**:
  - title
  - short summary
  - decisions
  - topics
  - action items, each with: text, owner, due date (if mentioned), and a source quote with a timestamp
- Owners are matched to named People. "I", "me" and "I'll" map to the user.
- Action items without a source quote are dropped, to limit invented tasks.
- The user can edit everything, and can regenerate the summary with a different model.

**Done when:** a 1-hour meeting is summarized on a 16 GB Mac with the default local model, and its action items link back to the correct moments in the transcript.

### Task 3: Linear integration

**3.1 Auth**
- A personal API key, entered in settings and stored through the `secrets` module (task 1.1). **(default)** OAuth is on the roadmap.

**3.2 Drafting tickets**
- Each action item becomes a draft ticket with:
  - title, and a description containing the quote and a link back to the meeting
  - team, project, priority and labels
  - an assignee:
    - matched by the person's email (from the calendar attendees) to a Linear user
    - otherwise picked by the user
    - the user by default
- The default team and project are remembered between meetings. **(default)**

**3.3 Approval and creation**
- A review list in the meeting detail view: edit, approve or discard each draft, or approve the selected ones at once.
- **Nothing is created without an explicit approval.**
- The created issue's ID is stored on the action item, so retrying never creates duplicates and the action item links to its issue.

**Done when:** approving drafts from a real meeting creates Linear issues with the correct assignees, and approving again creates nothing new.

### Task 4: bring your own keys

- Providers: OpenRouter (already partly present), Vercel AI Gateway, and Cloudflare Workers AI / AI Gateway. All are OpenAI-compatible, so each is a provider entry, a base URL and a key stored in the Keychain.
- A "Test connection" button, and a model list for each provider.
- Every feature from task 2 works with any provider. Cloud models with a large enough context skip the chunking step.

**Done when:** each new provider can produce a meeting summary, and no key ever appears in `settings_store.json`.

---

## 4. Permissions and Info.plist

These are added to `assets/Info.plist`. Each permission is requested the first time the user turns on meeting notes, not during onboarding.

| Key | Why |
|---|---|
| `NSAudioCaptureUsageDescription` | Record the other participants' audio (system audio). |
| `NSCalendarsFullAccessUsageDescription` | Find meetings and their attendees. |
| *(existing)* `NSMicrophoneUsageDescription` | The user's own voice. |

Notifications are requested through `UNUserNotificationCenter`.

---

## 5. Risks

1. **Voiceprint accuracy** on Meet-compressed audio and on overlapping speech. Task 1.0 tests this before any other work starts.
2. **Echo cancellation quality:** software AEC3 depends on the two tracks staying aligned and on the room. Residual echo is caught by the D9 text safety net.
3. **Small local models inventing action items.** Requiring a source quote for every item and the approval step in task 3 limit the damage.
4. **Mixing SQLCipher and plain SQLite.** The bundled `rusqlite` build has to support both the encrypted `meetings.db` and the unencrypted `history.db`. Confirm this at the start of task 1.5.
5. **Stable signing.** The Keychain and macOS privacy permissions depend on a stable code signature. Local builds use `scripts/package-macos.sh`, and release builds need a real signing identity.
