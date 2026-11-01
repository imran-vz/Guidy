# Meeting reliability scorecard

Goal: match Granola, the user's reliability standard. Granola publishes no reliability
figures, so these pass criteria are our own proposals, not Granola benchmarks.
"Granola" below is what its docs say or what its app bundle (7.722.1) shows.

Headline metric: **silent-failure rate is 0.** Every session either captures both
sources or shows a visible error during the call.

Run each scenario at least 10 times on macOS and log results in the table at the end.

| ID | Scenario | Pass criteria | Granola |
|---|---|---|---|
| R1 | Zoom app, no calendar event | Prompt <=5s after mic active; both sources captured | Mic-based prompt; timing unknown |
| R2 | Teams app | Same, with speaker names | Capture and tags documented |
| R3 | Meet in Chrome | Prompt <=5s; correct browser | Browser detection undocumented |
| R4 | Meet in Safari | Same | Same |
| R5 | Meet in Arc | Same | Same |
| R6 | Slack huddle, FaceTime, WhatsApp, Webex, Discord | Prompt <=5s, correctly titled | Huddle/Call prompts documented |
| R7 | Unknown site using the mic in a browser | Generic offer <=20s | Unknown |
| R8 | Calendar event with link and 2+ attendees | Prompt at T-60s +/-10s; correct title | 1 minute documented |
| R9 | 1-attendee event; voice note; dictation | No prompt; <=1 false prompt per 8h of use | Attendee filter documented |
| R10 | Ad-hoc call ends | Stop or prompt <=30s after mic release | Mechanism documented |
| R11 | Back-to-back calls | Two separate notes, >=95% of runs | Documented merging risk |
| R12 | AirPods connect or switch mid-call | Gap <=3s; visible notice; both sources resume | Unknown |
| R13 | Monitor unplug | System audio resumes <=3s | Auto-restart in changelog |
| R14 | Mic or system stream silently dead | Visible warning <=10s, 100% of runs | Bubbles and warning; timing unknown |
| R15 | Permission revoked mid-call | Specific error <=10s; no fake "recording" state | Unknown |
| R16 | Sleep 10 minutes, then wake | Session intact, no lost audio; recording resumes or stops visibly | Stops on sleep; manual resume |
| R17 | `kill -9` the app while recording | Relaunch recovers audio; loss <=5s before the kill | Unknown |
| R18 | Typed notes during `kill -9` | Loss <=1s; note reopens | Unknown |
| R19 | Network out during call and processing | No audio loss; processing retries or says why | Unknown beyond brief drops |
| R20 | 3-hour call | No stall; drift <=1s; bounded memory | Unknown |
| R21 | 15 minutes of silence | Warns before stopping; stop reason shown | 15-minute stop documented |
| R22 | Disk nearly full | Warning below 500 MB; audio so far stays readable | Unknown |

## Where Granola is weak and Guidy can beat it

- Granola keeps no audio, so gaps can't be re-transcribed. Guidy keeps CRC-checked chunks.
- Granola is cloud-only for transcription. Guidy works locally and offers cloud summaries.
- User reports (low confidence, via an aggregator) describe silent failures with no error.

## Results log

Fill this in on real calls. Only hardware runs count as passes.

| ID | Date | Build | Runs | Passes | Notes |
|---|---|---|---|---|---|
