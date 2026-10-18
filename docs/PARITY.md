# Parity matrix: Handy 0.9.8 (`9e57a0d`) → Guidy

Every row is covered by passing tests (`test: <name>`, a test function name) and/or a manual check (`manual: HW-xx`). `cargo xtask parity` verifies every referenced test exists, that all 300 baseline unit tests (`docs/baseline/`) survive, and that every baseline command and event has a counterpart.

Platform tags: **[mac] [win] [lin] [wl]** (Wayland). Rows without a tag apply to all platforms.

---

## C. Backend API (every Tauri command → `Backend` method)

`xtask parity` diffs the full list automatically against the baseline command list (`docs/baseline/commands.txt`). The rows below group them by behavior. Each `change_*_setting` needs a test that it persists, a test that it emits/applies its side effects, and a test that invalid input returns an error.

| ID | Group | Commands | Cover |
|---|---|---|---|
| C-01 | Bindings | change_binding, reset_binding, suspend_all_bindings, resume_all_bindings | test: reset_restores_default, test: binding_rollback_restores_the_original, test: system_recorder_suspends_bindings |
| C-02 | Keyboard impl | change_keyboard_implementation_setting (returns reset_bindings), get_keyboard_implementation | test: experimental_group_only_when_enabled, test: compound_shortcut_keys_parse_on_both_backends |
| C-03 | HandyKeys recorder | start_handy_keys_recording (fails with `secure-input-active`), stop_handy_keys_recording | test: handy_keys_recording_commits_on_key_release |
| C-04 | Simple setting setters | all ~50 `change_*_setting` in `shortcut/mod.rs` | test: simple_settings_persist_through_core, test: every_updatable_key_has_a_handler, test: invalid_values_are_rejected_not_written |
| C-05 | Theme | change_theme_setting → ThemeChanged event + window appearance | test: theme_change_emits_theme_changed, test: explicit_settings_ignore_the_os |
| C-06 | Debug / start / autostart / updates / what's-new setters → SettingsChanged event | | test: debug_mode_change_emits_settings_changed, test: sidebar_sections_follow_settings |
| C-07 | Post-process | set_post_process_provider, base_url/api_format/api_key/model setters, add/update/delete prompt, set selected prompt, fetch_post_process_models | test: create_prompt, test: api_rows, test: post_process_defaults_sync_builtin_api_format_but_keep_custom, test: anthropic_format_parses_messages_response |
| C-08 | Accelerators | get_available_accelerators, change_transcribe_accelerator_setting, change_transcribe_gpu_device | test: accelerator_encoding_round_trips, test: normal_hosts_preserve_every_transcribe_accelerator_setting |
| C-09 | Typing tools | get_available_typing_tools [lin] | test: paste_method_matrix, manual: HW-11 |
| C-10 | Models | get_available_models, get_model_info, rescan_local_models, download_model, cancel_download, delete_model, set_active_model, get_current_model, get_transcription_model_status, is_model_loading, get_retired_models, delete_retired_model | test: card_clicks_download_or_select, test: download_progress_and_cancel, test: sections_and_rescan, test: test_discover_custom_transcribe_models |
| C-11 | Audio | microphones list/select/get, output devices list/select/get, channels get/set, clamshell get/set, update/get_microphone_mode, play_test_sound, check_custom_sounds, is_recording | test: microphone_config_maps_the_selected_channel, test: channel_selector_only_for_multichannel_devices, test: debug_rows, manual: HW-02 |
| C-12 | Windows mic permission | get_windows_microphone_permission_status, open_microphone_privacy_settings [win] | test: missing_permissions_send_returning_users_to_permissions, manual: HW-01 |
| C-13 | Transcription | set_model_unload_timeout, get_model_load_status, unload_model_manually | test: model_unload_timeout_persists, test: unload_options_gate_sec15_on_debug |
| C-14 | History | get_history_entries(cursor, limit), toggle_history_entry_saved, get_audio_file_path, delete_history_entry, retry_history_entry_transcription, update_history_limit, update_recording_retention_period | test: entry_actions, test: pages_of_thirty, test: fresh_database_runs_all_migrations |
| C-15 | App | cancel_operation, is_portable, is_update_checks_locked (GUIDY_DISABLE_UPDATER), get_app_dir_path, get_log_dir_path, set_log_level, open_recordings_folder, open_log_dir, open_app_data_dir, get_app_settings, get_default_settings | test: paths_and_portable_flag, test: set_log_level_updates_file_filter_and_setting, test: about_rows |
| C-16 | Init | initialize_enigo, initialize_shortcuts (not run at startup; run after onboarding) | test: returning_user_initializes_input_and_devices |
| C-17 | Secure input [mac] | get_secure_input_status, run_keyboard_diagnostic(secs) | test: keyboard_diagnostic_runs_on_mac, test: secure_input_banner_rules, manual: HW-09 |
| C-18 | Misc | is_laptop, check_apple_intelligence_available, trigger_update_check, show_main_window_command | test: update_check_trigger_respects_setting, test: test_is_laptop, test: permission_onboarding_forces_window |

## E. Events (every `emit` → `CoreEvent` variant, consumer updates)

| ID | Event | Consumer behavior to verify | Cover |
|---|---|---|---|
| E-01 | recording-error {error_type, detail} | Toast: mic permission denied (per-platform text), no input device, generic | test: error_toasts_match_the_spec |
| E-02 | paste-error | Toast | test: error_toasts_match_the_spec |
| E-03 | transcription-error(String) | Toast with the payload | test: error_toasts_match_the_spec |
| E-04 | show-overlay / hide-overlay / recording-ready | Overlay state machine (§O) | test: compact_states_and_cancel, test: show_resets_capture_and_text_synchronously |
| E-05 | mic-level [f32;16] at ~30 fps | Overlay waveform | test: levels_are_smoothed_and_truncated |
| E-06 | stream-text-event / stream-phase-event | Live overlay | test: live_card_shows_text_and_timer, test: live_layout_states |
| E-07 | model-state-changed (unloaded, loading_started/failed/completed, selection_changed) | Settings refetch, status bar, overlay load notice, tray refresh, toast on failure | test: selector_status_transitions, test: slow_load_notice_only_after_2s_and_only_cleared_by_completion, test: error_toasts_match_the_spec |
| E-08 | model-download-progress / -complete / -failed / -cancelled, model-verification-started/-completed, model-deleted, models-updated | Models model state; auto-select 500 ms after completion if not recording | test: progress_tracks_percentage_and_speed, test: completion_clears_state_and_requests_reload_and_autoselect, test: failure_toasts_the_error, test: verification_toggles, test: reload_events, test: download_auto_select_waits_for_onboarding |
| E-09 | history-update-payload (added/updated/deleted/toggled) | Prepend, replace in place, ignore the rest | test: live_added_entries_prepend, test: live_events_prepend_and_replace |
| E-10 | settings-changed {setting, value, reset_bindings?} | Refetch; refresh devices when `selected_microphone` changes | test: debug_mode_change_emits_settings_changed, test: sidebar_sections_follow_settings |
| E-11 | theme-changed | Main window and overlay re-theme | test: theme_change_emits_theme_changed |
| E-12 | secure-input-changed | Banner | test: secure_input_banner_follows_events_and_dismisses_per_episode |
| E-13 | handy-keys-event | Shortcut recorder | test: handy_keys_recording_commits_on_key_release |
| E-14 | check-for-updates (from the tray) | Manual update check | test: tray_update_check_event_runs_a_manual_check |
| E-15 | log stream | Debug live log viewer | test: live_logs_buffer_and_clear, test: level_numbers_match_the_log_viewer |

## S. Startup, CLI, process

| ID | Behavior | Cover |
|---|---|---|
| S-01 | `--transcribe-worker` child process dispatch; supervisor restarts the worker | test: one_worker_at_a_time, test: crash_mid_stream_falls_back_to_batch, test: truncated_frame_is_an_error |
| S-02 | Headless `-f/--transcribe-file`, `--model`, `--device-index`, `--list-devices`, `--list-models`, `--repeat`, `--json`; exit codes; JSON matches the golden output | test: help_lists_the_cli_flags, test: list_models_json_is_the_model_registry, test: missing_wav_is_a_usage_error, test: transcription_matches_golden_output |
| S-03 | `--start-hidden`, `--no-tray`, `--debug` (forces debug mode + trace logging) | test: start_hidden_with_tray_hides_and_runs_as_accessory, test: start_hidden_without_tray_still_shows_window, test: help_lists_the_cli_flags |
| S-04 | Single instance: a second launch with `--toggle-transcription` / `--toggle-post-process` / `--cancel` forwards the action; with no flags it shows the window (and on mac recreates the tray) | test: second_launch_forwards_args_to_the_primary, test: forward_fails_without_a_primary, manual: HW-12, test: after_shutdown_a_new_launch_becomes_primary |
| S-05 | Signals: SIGUSR2 = transcribe; SIGUSR1 = post-process (all Unix: Handy skipped Linux only because of WebKitGTK) | manual: HW-12 |
| S-06 | Portable mode: `portable` marker file → `<exe>/Data`, HF_HOME, empty-marker upgrade [win] | test: test_valid_magic_string_enables_portable, test: test_hugging_face_home_is_inside_portable_data, manual: HW-08 |
| S-07 | Main window 680×570 min, starts hidden; shown unless start_hidden && tray available; shown if Windows mic is denied | test: shows_window_by_default, test: permission_onboarding_forces_window |
| S-08 | Close hides the window; Accessory activation policy when the tray is visible [mac] (a start-hidden launch *launches* as Accessory); Reopen shows it [mac] | manual: HW-13 |
| S-09 | Settings load: salvage when corrupt, all 8 migrations, new default bindings merged, written back | test: legacy_store_file_loads_and_migrates, test: salvage_preserves_valid_fields_when_one_value_is_invalid, test: frozen_v0_9_store_parses_strictly_then_migrates_device_index |
| S-10 | History DB: rusqlite_migration (4 steps), legacy `_sqlx_migrations` conversion, retention by count/age | test: fresh_database_runs_all_migrations, test: legacy_sqlx_database_is_adopted_without_rerunning_migrations, test: saving_emits_added_and_preserve_limit_keeps_saved_entries |
| S-11 | Logs: file 500 KB keep-one, level from settings; console filter from RUST_LOG | test: rotating_file_truncates_when_full, test: rotating_file_appends_to_existing |
| S-12 | Env: GGML_METAL_NO_RESIDENCY [mac], VK_LOADER_LAYERS_DISABLE [win], allocator tuning [lin] | manual: HW-10 |
| S-13 | Background warm-up of the accelerator list | manual: HW-10 |
| S-14 | Apple Intelligence Swift build + stub fallback [mac aarch64] | manual: HW-14 |

## T. Tray

| ID | Behavior | Cover |
|---|---|---|
| T-01 | Menu items: settings, check updates, copy last transcript, unload model, cancel (only while busy), model submenu `model_select:<id>`, quit; secure-input warning item | test: idle_menu_has_model_submenu_and_unload, test: busy_menu_offers_cancel_instead_of_models, test: secure_input_warning_is_slotted_below_version, manual: HW-04 |
| T-02 | Icon states: idle / recording / transcribing / warning, each with light and dark variants; updates on OS theme change | test: recording_and_transcribing_share_a_menu, test: tray_icon_returns_err_when_file_does_not_exist, manual: HW-04 |
| T-03 | Localized labels from the locale (exact → Chinese script → base → en) | test: localized_labels_follow_the_locale, test: resolves_locale_fallbacks |
| T-04 | `show_tray_icon` toggle; refresh on model-state-changed and on OS light/dark changes (also with no window open); Linux tray runs on its own GTK thread | test: unload_disabled_when_no_model_loaded_and_updates_disabled, test: app_group_rows_and_overlay_position_visibility, manual: HW-04 |

## R. Recording pipeline (core)

| ID | Behavior | Cover |
|---|---|---|
| R-01 | Shortcut activation: hold_or_toggle (hold threshold), push_to_talk, toggle | test: hold_or_toggle_autorepeat_burst_is_one_long_hold, test: genuine_release_after_grace_stops_recording_once, manual: HW-03 |
| R-02 | Cancel binding (not on [lin]); cancel_operation from overlay and tray | test: cancel_during_processing_drops_remembered_press, test: compact_states_and_cancel, manual: HW-03 |
| R-03 | VAD on/off, silero/earshot backends, extra recording buffer, long-form chunking | test: cuts_prefer_long_pauses_near_30s, test: silence_is_not_voice, test: scores_the_same_in_pieces_and_frame_by_frame |
| R-04 | Live/streaming transcription with lazy_stream_close | test: live_chunks_arrive_before_the_end_at_the_same_cuts, test: live_overlay_uses_streaming_states_only_for_streaming_models |
| R-05 | Always-on microphone, mute while recording, clamshell mic [mac laptop], channel selection/averaging | test: microphone_config_maps_the_selected_channel, test: test_clamshell_check, manual: HW-02 |
| R-06 | Audio feedback sounds (theme, volume, output device, custom_start/stop.wav) | test: debug_rows, manual: HW-02 |
| R-07 | Post-processing: providers incl. Apple Intelligence [mac], `${output}` prompt, transcribe_with_post_process binding | test: anthropic_request_carries_required_max_tokens, test: openrouter_uses_nested_reasoning_object, test: leading_think_block_is_stripped, manual: HW-14 |
| R-08 | Text pipeline: custom words + correction threshold, filler removal, Chinese script, translate to English, language selection/effective language, trailing space | test: fuzzy_correction_is_skipped_exactly_when_vocabulary_was_sent, test: auto_detected_chinese_text_is_converted, test: transcribe_cpp_run_plan_translates_supported_non_english, test: unknown_evidence_with_confident_text_detection_removes_gated_fillers |
| R-09 | Output: paste methods ctrl_v/direct/none/ctrl_shift_v/shift_insert/external_script, typing tool [lin], reliable paste [mac win], paste delays, clipboard handling, auto-submit keys | test: auto_submit_runs_for_active_paste_methods, test: clipboard_is_restored_before_key_injection_error_is_returned, test: finishes_after_quiet_period_once_read, manual: HW-02 |
| R-10 | History entry saved with WAV; retention applied | test: saving_emits_added_and_preserve_limit_keeps_saved_entries, test: get_latest_completed_entry_skips_empty_entries |
| R-11 | Model idle unload (timeouts incl. sec15 in debug), manual unload | test: model_unload_timeout_persists, test: unload_options_gate_sec15_on_debug, manual: HW-02 |
| R-12 | Secure input detection + Carbon fallback hotkeys [mac] | test: secure_input_banner_rules, manual: HW-09 |

## O. Overlay window

| ID | Behavior | Cover |
|---|---|---|
| O-01 | Non-activating, always on top, transparent, on the cursor's monitor; top/bottom offsets (mac 46/15, others 4/40); Windows text scale; LayerShell anchored to the edge [wl] (`GUIDY_NO_LAYER_SHELL=1` opts out) | test: overlay_is_centered_and_offset_from_the_chosen_edge, test: cursor_selects_the_secondary_monitor, test: monitor_hit_test_uses_half_open_physical_bounds, manual: HW-05 |
| O-02 | overlay_style none / minimal / live; size 256×50 compact, 400×120 streaming | test: streaming_state_uses_the_large_size, test: compact_states_and_cancel |
| O-03 | Compact: recording row (dot, waveform, cancel), working row (spinner, label, cancel); width 172→216; 200 ms fade | test: compact_states_and_cancel, test: working_labels |
| O-04 | Waveform: 16 buckets smoothed prev·0.7+new·0.3, first 9 shown, h = clamp(3, 3+v^0.7·15, 18); arming state muted | test: bar_heights_and_timer, test: levels_are_smoothed_and_truncated |
| O-05 | Status dot accent pulse when capture is ready; muted while arming | test: show_resets_capture_and_text_synchronously |
| O-06 | Live card: pill 184 → panel 392 / working pill 216; pop-in 460 ms; text open 440 ms; committed + tentative + caret; max height 64 | test: live_card_shows_text_and_timer, test: live_layout_states, test: streaming_session_bumps_and_resets_phase |
| O-07 | Auto-follow newest text unless scrolled up (re-pin within 16 px); top placement reverses the order; fade mask | test: bar_heights_and_timer, test: streaming_session_bumps_and_resets_phase |
| O-08 | Timer m:ss counts only while streaming, visible and capture ready | test: timer_counts_from_capture_ready_while_streaming, test: live_card_shows_text_and_timer |
| O-09 | Working label: processing (polishing) / loadingModel / transcribing; slow-load notice after 2000 ms, cleared only by completed/failed | test: working_labels, test: slow_load_notice_only_after_2s_and_only_cleared_by_completion |
| O-10 | Show resets state synchronously; language resync on show; theme follows the setting | test: show_resets_capture_and_text_synchronously, test: theme_change_emits_theme_changed |
| O-11 | Cancel button → cancel_operation | test: compact_states_and_cancel |

## W. Main window shell

| ID | Behavior | Cover |
|---|---|---|
| W-01 | Sidebar: general, history, models, advanced, postprocessing (if post_process_enabled), debug (if debug_mode), about | test: sidebar_sections_follow_settings, test: sidebar_visibility |
| W-02 | Cmd/Ctrl+Shift+D toggles debug_mode | test: debug_shortcut_toggles_debug_mode |
| W-03 | Scroll resets to top when the section changes | test: navigation_switches_pages |
| W-04 | Accessibility-missing banner [mac] (request → verify → recheck) | test: accessibility_banner_when_permission_missing |
| W-05 | Secure input banner [mac]: shown when sustained && degraded/uncovered, or recorder_blocked; plural variants; culprit name; learn more; dismiss for this episode only | test: secure_input_banner_follows_events_and_dismisses_per_episode, test: secure_input_banner_rules |
| W-06 | Status bar: model selector (left); update status • version (right) | test: status_bar_shows_model_and_version |
| W-07 | Optimistic setting update with isUpdating spinner and rollback on error; reset to default | test: failed_setting_rolls_back, test: optimistic_update_applies_and_commits, test: reset_uses_defaults |
| W-08 | Settings row: title + description inline or as a tooltip; disabled state | test: shortcut_rows, test: output_device_and_volume_follow_audio_feedback |

## G. General page

| ID | Behavior | Cover |
|---|---|---|
| G-01 | Shortcut recorder `transcribe` | test: shortcut_rows, test: handy_keys_recording_commits_on_key_release |
| G-02 | shortcut_activation select with descriptions | test: shortcut_rows |
| G-03 | Shortcut recorder `cancel` (hidden on [lin]) | test: shortcut_rows |
| G-04 | Model settings card only when the model supports language selection or translation | test: model_settings_card_follows_model_capabilities |
| G-05 | Language selector: searchable, Enter picks first, Esc closes, filtered to supported languages, Auto only if detection is supported, shows the effective language, aliases nb→no and fil→tl, reset | test: model_settings_card_follows_model_capabilities, test: effective_language_resolution, test: available_languages_gate_auto_on_detection, test: labels_and_filtering |
| G-06 | translate_to_english toggle if supported | test: model_settings_card_follows_model_capabilities |
| G-07 | Microphone select (Default + devices, refresh on open, reset) | test: returning_user_initializes_input_and_devices, test: shortcut_rows, test: device_list_failure_falls_back_to_default, test: device_reset_means_the_system_default |
| G-08 | Channel select only if channels > 1; average when out of range | test: channel_selector_only_for_multichannel_devices |
| G-09 | mute_while_recording, audio_feedback toggles | test: audio_feedback_toggle_persists_through_the_backend, test: failed_setting_rolls_back |
| G-10 | Output device select + volume slider (0–1, %) disabled unless audio_feedback | test: output_device_and_volume_follow_audio_feedback |

## A. Advanced page

| ID | Behavior | Cover |
|---|---|---|
| A-01 | start_hidden, autostart_enabled, show_tray_icon toggles | test: app_group_rows_and_overlay_position_visibility |
| A-02 | overlay_style select; overlay_position hidden when style = none | test: app_group_rows_and_overlay_position_visibility |
| A-03 | model_unload_timeout options (sec15 only in debug) via set_model_unload_timeout | test: unload_options_gate_sec15_on_debug, test: model_unload_timeout_persists |
| A-04 | experimental_enabled toggle | test: experimental_group_only_when_enabled |
| A-05 | paste_method option matrix per OS (direct hidden on mac unless already saved → shown disabled; ctrl_shift_v/shift_insert on win/lin; external_script on lin + path input) | test: paste_method_matrix |
| A-06 | typing_tool select [lin] when paste_method = direct | test: paste_method_matrix, manual: HW-11 |
| A-07 | clipboard_handling select | test: simple_settings_persist_through_core |
| A-08 | Auto-submit select maps to the auto_submit + auto_submit_key pair; Cmd vs Super labels | test: auto_submit_mapping, test: auto_submit_writes_run_in_order |
| A-09 | vad_enabled, long_form_chunking, filler_word_removal toggles; chinese_script select | test: transcription_and_history_rows |
| A-10 | Custom words: normalize (strip <>"', collapse whitespace, max 50), Enter adds, duplicate toast, click chip removes | test: custom_words_add_and_remove, test: custom_words_normalize_and_dedupe |
| A-11 | append_trailing_space toggle | test: transcription_and_history_rows |
| A-12 | history_limit number 0–1000; retention select (preserve_limit label shows the limit) | test: transcription_and_history_rows, test: saving_emits_added_and_preserve_limit_keeps_saved_entries |
| A-13 | Experimental group: post_process_enabled, keyboard implementation (reset-bindings warning toast), acceleration select (writes gpu device, then accelerator), lazy_stream_close, vad_backend (error toast + rollback) | test: experimental_group_only_when_enabled, test: accelerator_encoding_round_trips, test: accelerator_writes_run_in_order |

## M. Models page and model selector

| ID | Behavior | Cover |
|---|---|---|
| M-01 | Search (name + description), rescan button, streaming/translation filters, language filter popover | test: sections_and_rescan, test: filter_hides_undownloaded_legacy_and_applies_toggles |
| M-02 | Downloaded vs available sections; sort active first, custom last; hide undownloaded legacy Url models; empty state | test: sections_sort_active_first_custom_last, test: sections_and_rescan |
| M-03 | Card status precedence: verifying → downloading → switching → downloadable → active → available | test: card_status_precedence, test: card_clicks_download_or_select |
| M-04 | Card content: badges (recommended/active/custom/legacy/switching), accuracy and speed bars, language count or "only X", translate/streaming tags, size, quantization in debug | test: sizes_and_quantization, test: sections_and_rescan |
| M-05 | Download progress %, MB/s (EMA 0.8/0.2, updated every ≥0.5 s), cancel; verifying state | test: download_speed_is_smoothed_and_throttled, test: download_progress_and_cancel |
| M-06 | Delete with confirmation dialog (different text when the model is active) | test: delete_asks_for_confirmation |
| M-07 | Retired models notice: total size, delete all, result toast | test: retired_notice_deletes_all_but_current |
| M-08 | Status bar selector: status dot (ready/loading/downloading/verifying/error/unloaded/none), text priority, dropdown of downloaded models, optimistic pending selection | test: selector_status_transitions, test: status_bar_shows_model_and_version, test: selector_status_follows_the_current_model |
| M-09 | Multi-download display | test: progress_tracks_percentage_and_speed |

## H. History page

| ID | Behavior | Cover |
|---|---|---|
| H-01 | Cursor pagination of 30, infinite scroll, dedupe by id | test: pages_of_thirty, test: pages_append_without_duplicates |
| H-02 | Open recordings folder | test: history_open_folder |
| H-03 | Row: localized date (seconds timestamp), selectable text, "transcription failed" when empty | test: entry_actions, test: history_dates_are_localized |
| H-04 | Copy (check icon for 2 s, disabled when empty, error toast) | test: copy_writes_clipboard_and_is_disabled_for_empty_text |
| H-05 | Star toggle, optimistic with revert | test: entry_actions, test: optimistic_toggle_and_remove |
| H-06 | Re-transcribe with spinner/pulse; result arrives through the updated event | test: entry_actions |
| H-07 | Delete, optimistic; reload on failure | test: entry_actions |
| H-08 | Audio player: lazy load, autoplay, play/pause, m:ss, seek with drag; only one player plays at a time | test: reads_wav_duration, manual: HW-15 |

## P. Post-processing page

| ID | Behavior | Cover |
|---|---|---|
| P-01 | transcribe_with_post_process shortcut recorder | test: api_rows |
| P-02 | Provider select; Apple Intelligence availability check + unavailable alert [mac] | test: api_rows, manual: HW-14 |
| P-03 | Auto-fetch models after a provider switch when a key (or custom base URL) exists | test: post_process_rules |
| P-04 | Custom provider: API format select (clears model cache), base URL saved on blur (resets model) | test: post_process_rules, test: base_url_change_clears_the_model |
| P-05 | API key password field saved on blur; hidden for Apple | test: api_rows |
| P-06 | Model: creatable select + refresh with spinner | test: api_rows, test: post_process_rules |
| P-07 | Prompts: select, create, edit (update only when dirty), delete (disabled when only one is left), empty-state hints | test: create_prompt, test: post_process_rules, test: created_prompt_stays_selected |

## D. Debug page

| ID | Behavior | Cover |
|---|---|---|
| D-01 | log_level select | test: debug_rows |
| D-02 | What's New preview (toast if there are no notes); onboarding preview (permissions/models) with exit button | test: onboarding_preview_is_inert_and_exits, test: release_note_selection |
| D-03 | update_checks_enabled disabled and shown off when locked | test: debug_rows |
| D-04 | Sound theme select (+custom when both files exist), play test start → stop | test: debug_rows |
| D-05 | Sliders with reset: word_correction_threshold, paste delays, hold_threshold_ms, extra_recording_buffer_ms (ranges per inventory) | test: debug_rows, test: slider_values_snap_to_step |
| D-06 | reliable_paste [mac win], always_on_microphone, clamshell microphone (laptops only) | test: debug_rows |
| D-07 | Keyboard diagnostic [mac] with verdicts | test: keyboard_diagnostic_runs_on_mac, test: keyboard_verdicts |
| D-08 | Live log viewer: 250 ms flush, 1000-line cap, pause, copy ("HH:MM:SS LEVEL msg"), clear, pin to bottom within 24 px, level colors | test: live_logs_buffer_and_clear, test: log_buffer_caps_and_formats |

## B. About page

| ID | Behavior | Cover |
|---|---|---|
| B-01 | App language select (native + English names) applies immediately | test: about_rows, test: all_26_locales_are_embedded_with_metadata |
| B-02 | Theme select system/light/dark applies immediately | test: about_rows, test: system_follows_the_os |
| B-03 | Version; show_whats_new_on_update toggle | test: about_rows |
| B-04 | App data dir and log dir paths with Open buttons | test: about_rows |
| B-05 | Acknowledgments (ggml). Handy's donate/source links are replaced by Guidy's own links (branding) | test: about_rows |

## F. Flows

| ID | Behavior | Cover |
|---|---|---|
| F-01 | Startup step machine: returning vs new user; permission checks [mac]/[win]; migration; model; done → init enigo + shortcuts + device refresh | test: new_user_goes_through_permissions_then_model_picker, test: returning_user_initializes_input_and_devices, test: returning_users_skip_to_done_or_migration, test: startup_initializes_dictation_without_a_window, test: reopening_the_window_keeps_the_session_state |
| F-02 | Permission onboarding: per-permission states, 1 s polling, stop after 3 errors with toast, 300 ms completion; [win] mic-only path; other platforms skip | test: permission_grant_polls_until_granted, test: new_user_goes_through_permissions_then_model_picker |
| F-03 | Model picker: existing models, 2 featured recommended, show all/fewer, download → select, cancel, back when opened from migration, cards disabled during selection | test: picker_downloads_then_selects_and_finishes, test: picker_groups_feature_two_recommended, test: picker_unlocks_after_a_failed_download |
| F-04 | Retired-model migration: choose phase, parallel downloads with progress/retry, select target before deleting old, ETA + MB/s, choose another | test: retired_model_migration_replaces_and_finishes, test: full_flow_selects_target_before_deleting_and_wraps_up, test: failed_target_can_retry, test: migration_failed_download_offers_retry |
| F-05 | What's New gate: highest note ≤ app version and > last seen; dismiss saves; markdown allowlist (http/https/mailto links, safe images) | test: shows_unseen_note_and_records_dismissal, test: respects_the_toggle, test: note_links_are_allowlisted, test: whats_new_waits_for_onboarding |
| F-06 | Updater: auto check on start, manual via tray, statuses, download progress, install + relaunch; disabled/locked states | test: manual_check_shows_up_to_date_then_auto_check_does_not, test: available_update_installs_with_progress_text, manual: HW-07, test: builds_without_an_update_endpoint_are_locked |
| F-07 | Portable update: dialog with the windows-<arch>-nsis URL or a releases fallback | test: portable_installer_url_resolution |

## K. Keyboard logic (ported from `keyboard.ts`)

| ID | Behavior | Cover |
|---|---|---|
| K-01 | get_key_name mapping (code first; modifiers per OS; named + punctuation maps; fallbacks) incl. the ported keyboard.test.ts cases | test: compound_keys_store_compact_and_display_spaced, test: key_fallback_and_unknown_codes, test: os_specific_modifiers |
| K-02 | format_key_combination (Left/Right, compound names, upper-case single chars, `fn`) | test: formats_sides_function_keys_and_fn |
| K-03 | System recorder: suspend bindings, record until all released, modifiers first, error toast + restore, resume; click outside cancels | test: system_recorder_suspends_bindings, test: system_recorder_commits_when_all_keys_are_released, test: system_recorder_resumes_after_the_binding_write, test: closing_while_recording_resumes_bindings |
| K-04 | HandyKeys recorder: commit on main-key release; modifier-only commits after full release without a main key (#1578); secure-input-blocked toast | test: handy_keys_recording_commits_on_key_release, test: handy_keys_modifier_only_after_full_release |

## I. i18n, theme, accessibility

| ID | Behavior | Cover |
|---|---|---|
| I-01 | 26 locales, no missing or extra keys vs en, no brand string | test: locales_have_no_brand_leftovers_and_match_english_keys, test: english_has_the_full_key_set |
| I-02 | Language resolution (exact → base; zh-Hant/TW/HK/MO → zh-TW; yue) and OS locale fallback | test: language_resolution_follows_locale_rules |
| I-03 | RTL for ar/he | test: rtl_languages, manual: HW-16 |
| I-04 | Plurals (secure input messages) | test: plurals_select_one_and_other |
| I-05 | One Dark/One Light themes, follows the OS in system mode | test: system_follows_the_os, test: explicit_settings_ignore_the_os, test: embeds_fonts_theme_and_notes |
| I-06 | Accessible names on all controls | test: sidebar_sections_follow_settings, manual: HW-17 |

## HW. Manual hardware checks (signed off per release)

| ID | Check | Platforms |
|---|---|---|
| HW-01 | First-run mic + accessibility permission prompts | mac, win |
| HW-02 | Dictate into TextEdit/Notepad/gedit, Chrome, Terminal with each paste method | all |
| HW-03 | Hold / toggle / push-to-talk / cancel on a physical keyboard | all |
| HW-04 | Tray menu + model switching; icon states in light and dark menu bar | all |
| HW-05 | Overlay top/bottom on a secondary monitor; does not steal focus | all, wl |
| HW-06 | Autostart after reboot | all |
| HW-07 | Update N → N+1 via the updater | all |
| HW-08 | Portable mode from a USB folder | win |
| HW-09 | Secure input banner (Terminal Secure Keyboard Entry) + fallback hotkeys | mac |
| HW-10 | GPU acceleration selection (Metal / Vulkan) and CPU fallback | all |
| HW-11 | Wayland global shortcut + paste via wtype/dotool | wl |
| HW-12 | Remote control: `guidy --toggle-transcription` / `--toggle-post-process` / `--cancel` to a running instance; SIGUSR2 (and SIGUSR1 on macOS) | all (signals: unix) |
| HW-13 | Close button hides into the tray (no Dock icon on macOS with tray), relaunch / Dock click reopens and restores a vanished tray icon | all |
| HW-14 | Apple Intelligence post-processing on a supported Apple Silicon Mac; unavailable notice elsewhere | mac |
| HW-15 | History playback: play / pause / seek; starting one recording stops another | all |
| HW-16 | Arabic / Hebrew UI renders right-to-left | all |
| HW-17 | Screen reader (VoiceOver / Narrator / Orca) announces settings rows and controls | all |
