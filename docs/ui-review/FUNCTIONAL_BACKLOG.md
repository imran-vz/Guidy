# Functional backlog (not worked on in the UI refresh)

Things the review council found that change behaviour, data or flows rather
than presentation. Recorded for later; the UI refresh does not touch them.
Source in brackets: `O` = opus report, `C` = codex report, `I` = noticed
while implementing. Round of discovery after the source.

## Bugs or likely bugs

1. Model descriptions repeat information ("Very fast, English only. · English only", "Supports 25 European languages. · 25 languages") in the picker and Models. `kit::model_description` concatenation. (O0, C0)
2. Usage error state leaves "Clear Usage…" enabled while the database is locked; Storage error leaves Refresh and Try Again doing the same thing. (O0)
3. A model marked "Required by Dictation · Meetings" still shows an enabled delete button. (O0)
4. "Generate summary" is offered while a recording is still running; define whether it summarizes so far or waits. (O0, C0)
5. Finish is enabled in cloud onboarding with an empty API key; decide whether incomplete setup is allowed and what state it opens into. (C0)
6. Onboarding Permissions makes "Continue Without" the default (Enter) action although Accessibility is needed for auto-paste. (O0)
7. The overlay timer reads 0:00 while text is mid-sentence (may be snapshot timing; verify the timer starts with audio). (O0)
8. Meetings fixture: every older meeting starts at 14:30; check list time formatting against real start times. (O0)
9. General shows both "Dictation key" and "Transcribe shortcut" set to Right Option; verify precedence and naming. (C0)
10. History offers retranscription while the fixture reports "Recording not found"; verify unavailable actions are disabled with an explanation. (C0)

## Product and flow ideas

1. Information architecture: one home per setting. Sound Theme, Check for Updates and paste delays live in Debug; Launch on Startup and Overlay in Advanced; API keys are set in both Post Process and Meetings › AI Summaries. Consider moving user-facing settings out of Debug and one AI-provider store. (O0, C0)
2. A separate preferences window (library-focused main window), which would change navigation, window persistence and shortcuts. (C0)
3. Move People into the sidebar and Meeting Settings into the app settings, removing the Meetings segmented control. (O0)
4. "Check for updates" appears in the status bar, About and Debug: keep one action plus one preference. (O0)
5. Post Process shows "Select a prompt to edit it." with one prompt present; auto-select the first prompt. (O0)
6. Post Process Model has both a free-text field and a "Choose" pop-up for the same value. (O0)
7. History Limit and Auto-Delete Recordings overlap and their confirm copy disagrees ("Keeping only 2"). (O0)
8. The AI Summaries pane shows the default model in two lists (saved models and catalogue); merge them. (O0)
9. Distinguish configured, default, downloaded and ready models in the state model so labels can be accurate. (C0)
10. Startup error: a supported retry or data-location recovery for an unwritable history folder. (C0)
11. Person merge: confirmation, affected-meeting scope and undo. (C0)
12. Speaker thresholds expose raw similarity numbers; consider labeled "Stricter … Looser" sliders (behaviour unchanged). (O0)
13. Storage has no ⌘ shortcut because Debug takes ⌘8. (O0)
14. Search placement differs: History search in the toolbar, Meetings and People search at the top of the list. (O0)
15. Keyboard and assistive-technology audit end to end: tab order, roles, focus retention after resizing, actual hit bounds. (C0)
16. Measure animation and idle power during recording and downloads. (C0)
17. A "Show Sidebar" toggle and sidebar collapse below ~760 pt. (O0, spec)

## Found during implementation

1. HUD controls (recording pill, expanded panel, overlay Cancel) are not keyboard-focusable; a focus ring needs focusability first. (I2)
2. The AI pane's catalogue scroll trap blocks wheel scrolling of the outer pane while the list can scroll. (I1)
3. The overlay's arming ripple and caret blink are the remaining repeating animations (Reduce Motion honoured). (I2)
4. `format_model_size` divides by 1024 but labels "GB" while `format_bytes` uses 1000, so a model's size can differ between pages; the AI pane's local rows now use the same formatter as their Download button. (I5)
5. In History's narrow detail view ⌘F returns to the list to focus search (the search field is hidden there). (I5)
6. Storage error path: if the data folder truly can't be located, the path query may fail too and no path shows. (I5)
