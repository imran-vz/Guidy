#ifndef meeting_bridge_h
#define meeting_bridge_h

// C ABI between `swift/meeting_bridge.swift` and
// `src/meetings/native/macos.rs` (meeting notes: EventKit, microphone, Core
// Audio process tap, user notifications). Every string returned by the bridge
// is malloc'd and must be released with `guidy_mb_free_string`.

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Authorization states shared by calendar / system audio / notifications.
#define GUIDY_MB_AUTH_NOT_DETERMINED 0
#define GUIDY_MB_AUTH_RESTRICTED 1
#define GUIDY_MB_AUTH_DENIED 2
#define GUIDY_MB_AUTH_GRANTED 3
#define GUIDY_MB_AUTH_WRITE_ONLY 4
#define GUIDY_MB_AUTH_UNKNOWN 5
#define GUIDY_MB_AUTH_UNAVAILABLE 6

// Stream ids passed to the capture callbacks.
#define GUIDY_MB_STREAM_MIC 0
#define GUIDY_MB_STREAM_SYSTEM 1

void guidy_mb_free_string(char *s);

#ifdef __OBJC__
// Swift-internal (objc_exception.m): runs `block`, returning 0, or 1 when it
// raised an Objective-C exception, with `*reason` set to a malloc'd
// description (release with `free`).
int guidy_mb_objc_try(void (^__attribute__((noescape)) block)(void), char **reason);
#endif

// 1 on macOS 14.2+ (Core Audio process taps), else 0.
int32_t guidy_mb_is_supported(void);

// ----- Calendar (EventKit) -------------------------------------------------
int32_t guidy_mb_calendar_auth_status(void);
// Blocks until the user answers. 1 = full access granted.
int32_t guidy_mb_request_calendar_access(void);
// Returns a JSON array of raw events (see `RawEvent` in Rust), or NULL with
// `*error_out` set.
char *guidy_mb_events_between(int64_t start_ms, int64_t end_ms, char **error_out);

// ----- Capture ---------------------------------------------------------------
// `samples` is 16 kHz mono f32, valid only for the duration of the call.
typedef void (*guidy_mb_audio_cb)(void *ctx, int32_t stream, const float *samples,
                                  int32_t count, int32_t sample_rate);
// Non-fatal or fatal stream problems (device change, restart failure, ...).
typedef void (*guidy_mb_event_cb)(void *ctx, int32_t stream, const char *message);

// Starts the requested streams. Returns an opaque handle, or NULL with
// `*error_out` set (nothing is left running in that case).
//
// Timeline: both streams share one time origin, the moment this function is
// called. Sample n of either stream (counted from its first callback) was
// captured n / 16000 seconds after that origin, within ~50 ms. Gaps (late
// start, mic restarts after a device or configuration change, a process tap
// that is quiet while no app plays audio) are filled with silence, and
// overlapping audio is dropped, so the two tracks can be laid side by side
// without further alignment. Callbacks never block on anything but a short
// internal lock.
void *guidy_mb_capture_start(int32_t want_mic, int32_t want_system, guidy_mb_audio_cb audio_cb,
                             guidy_mb_event_cb event_cb, void *ctx, char **error_out);
// Stops and frees the handle. After it returns no callback is running or will
// run for this handle.
void guidy_mb_capture_stop(void *handle);

// System-audio (process tap) permission. Best effort: uses the private TCC
// preflight when available (run off the main thread, at most ~2 s), else
// returns UNKNOWN. Any unexpected result is UNKNOWN.
int32_t guidy_mb_system_audio_status(void);
// Triggers the TCC prompt and returns the resulting status. BLOCKS until the
// user answers, for up to ~60 s (then the preflight status is returned). Call
// it from a background thread: on the main thread it only shows the prompt
// and returns the current status without waiting. Without the private TCC
// request API it runs a ~1.5 s probe tap instead (DENIED if the tap can't
// start, GRANTED if it hears audio, else the preflight status).
int32_t guidy_mb_request_system_audio_access(void);

// ----- Test hooks (unit tests of the timeline aligner) ----------------------
// A standalone `TimelineAligner` (16 kHz). `audio`/`tick` copy what the aligner
// emits (silence and audio) into `out` (up to `capacity` samples; `out` may be
// NULL) and return the number of samples emitted.
void *guidy_mb_test_aligner_new(uint64_t start_nanos, int64_t tolerance_samples);
int64_t guidy_mb_test_aligner_audio(void *aligner, const float *samples, int32_t count,
                                    uint64_t at_nanos, float *out, int64_t capacity);
int64_t guidy_mb_test_aligner_tick(void *aligner, uint64_t now_nanos, float *out,
                                   int64_t capacity);
void guidy_mb_test_aligner_free(void *aligner);

// ----- Notifications ---------------------------------------------------------
typedef void (*guidy_mb_notification_cb)(const char *notification_id, const char *action_id);

// 0 when there is no bundle identifier (unbundled binary).
int32_t guidy_mb_notifications_available(void);
int32_t guidy_mb_notification_auth_status(void);
// Blocks until the user answers. 1 = granted.
int32_t guidy_mb_request_notification_access(void);
// `actions_json` is a JSON array of [action_id, title] pairs. Returns 1 when the
// request was handed to the notification center.
int32_t guidy_mb_post_notification(const char *notification_id, const char *title,
                                   const char *body, const char *actions_json);
void guidy_mb_remove_notification(const char *notification_id);
void guidy_mb_set_notification_handler(guidy_mb_notification_cb cb);

// ----- Calls in other apps ---------------------------------------------------
// A JSON array of the bundle ids of the other processes capturing from an
// input device now (macOS 14+; `[]` before), or NULL.
char *guidy_mb_audio_input_bundle_ids(void);
// A JSON array of the window titles of the running apps with `bundle_id`
// (Accessibility; `[]` without it), or NULL.
char *guidy_mb_window_titles(const char *bundle_id);

#ifdef __cplusplus
}
#endif

#endif /* meeting_bridge_h */
