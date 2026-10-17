#ifndef meeting_bridge_h
#define meeting_bridge_h

// C ABI between `swift/meeting_bridge.swift` and
// `src/meetings/native/macos.rs` (meeting notes: EventKit, VPIO mic, Core
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
void *guidy_mb_capture_start(int32_t want_mic, int32_t want_system, guidy_mb_audio_cb audio_cb,
                             guidy_mb_event_cb event_cb, void *ctx, char **error_out);
// Stops and frees the handle. After it returns no callback is running or will
// run for this handle.
void guidy_mb_capture_stop(void *handle);

// System-audio (process tap) permission. Best effort: uses the TCC preflight
// when available, else returns UNKNOWN.
int32_t guidy_mb_system_audio_status(void);
// Triggers the TCC prompt (blocking). Returns the resulting status.
int32_t guidy_mb_request_system_audio_access(void);

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

#ifdef __cplusplus
}
#endif

#endif /* meeting_bridge_h */
