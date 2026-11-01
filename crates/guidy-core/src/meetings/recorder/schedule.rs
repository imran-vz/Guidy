//! Pure timing and event-selection rules (D16 reminders, calendar prompts,
//! manual-start linking). No I/O, so every rule is unit-tested here.

use crate::meetings::types::CalendarEvent;

pub const MINUTE_MS: i64 = 60_000;
/// First reminder for a calendar meeting: this long after its scheduled end.
pub const AFTER_SCHEDULED_END_MS: i64 = 5 * MINUTE_MS;
/// First reminder for a manual session (no scheduled end).
pub const MANUAL_FIRST_REMINDER_MS: i64 = 60 * MINUTE_MS;
/// A calendar prompt is still posted this long after the event's start (the
/// Mac may have been asleep briefly).
pub const PROMPT_WINDOW_MS: i64 = 2 * MINUTE_MS;
/// A calendar prompt is posted this long before the event's start. The
/// scheduler ticks every 15 s, so it lands 50-65 s before: one minute, give or
/// take the 10 s the scorecard allows.
pub const PROMPT_LEAD_MS: i64 = 65_000;
/// How far ahead the calendar is read: past the lead, as it's read once a
/// minute.
pub const PROMPT_LOOKAHEAD_MS: i64 = 3 * MINUTE_MS;
/// Fewer attendees than this is not a meeting to prompt for (a personal block
/// with a link, a note to self).
pub const PROMPT_MIN_ATTENDEES: usize = 2;
/// A start links to a call event beginning within this long.
pub const UPCOMING_LINK_MS: i64 = 5 * MINUTE_MS;
/// Events at least this long are all-day / multi-day and never prompt.
const ALL_DAY_MS: i64 = 20 * 60 * MINUTE_MS;

/// What reminder timing depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderState {
    pub started_at: i64,
    /// End of the linked calendar event; `None` for a manual session.
    pub scheduled_end_at: Option<i64>,
    /// When the last reminder was posted (or "Keep going" was pressed).
    pub last_reminder_at: Option<i64>,
    pub interval_minutes: u32,
}

/// When the next "still recording" reminder is due (unix ms).
///
/// - Calendar meeting: 5 min after the scheduled end, then every interval.
/// - Manual session: 60 min after the start, then every interval.
pub fn next_reminder_at(state: &ReminderState) -> i64 {
    let interval = state.interval_minutes.max(1) as i64 * MINUTE_MS;
    match state.last_reminder_at {
        Some(last) => last + interval,
        None => match state.scheduled_end_at {
            Some(end) => end + AFTER_SCHEDULED_END_MS,
            None => state.started_at + MANUAL_FIRST_REMINDER_MS,
        },
    }
}

pub fn reminder_due(now: i64, state: &ReminderState) -> bool {
    now >= next_reminder_at(state)
}

fn is_all_day(e: &CalendarEvent) -> bool {
    e.end_at - e.start_at >= ALL_DAY_MS
}

/// The event a manual start links to: one happening at `now`, preferring
/// events with a meeting link, then the most recently started.
pub fn current_event(events: &[CalendarEvent], now: i64) -> Option<&CalendarEvent> {
    events
        .iter()
        .filter(|e| e.start_at <= now && now < e.end_at && !is_all_day(e))
        .max_by_key(|e| (e.meeting_url.is_some(), e.start_at))
}

/// With nothing on now, the call event a start at `now` belongs to: one with a
/// meeting link about to begin, else the one that ended most recently, within
/// [`START_LINK_GRACE_MS`] (calls run over).
pub fn nearby_call_event(events: &[CalendarEvent], now: i64) -> Option<&CalendarEvent> {
    let calls = || {
        events
            .iter()
            .filter(|e| e.meeting_url.is_some() && !is_all_day(e))
    };
    calls()
        .filter(|e| e.start_at > now && e.start_at - now <= UPCOMING_LINK_MS)
        .min_by_key(|e| e.start_at)
        .or_else(|| {
            calls()
                .filter(|e| e.end_at <= now && now - e.end_at <= START_LINK_GRACE_MS)
                .max_by_key(|e| e.end_at)
        })
}

/// How long after its event ends a start for that event (a calendar prompt's
/// "Start" clicked late, a call detected as it ran over) still links the
/// recording to it.
pub const START_LINK_GRACE_MS: i64 = 15 * MINUTE_MS;

/// Whether a start for `e` at `now` links to it: not once it ended more than
/// [`START_LINK_GRACE_MS`] ago.
pub fn links_to_event(e: &CalendarEvent, now: i64) -> bool {
    now <= e.end_at + START_LINK_GRACE_MS
}

/// Events about to start that should get a "Take notes for …?" prompt: they
/// have a meeting link and [`PROMPT_MIN_ATTENDEES`], aren't all-day, and start
/// within [`PROMPT_LEAD_MS`] or started within the last [`PROMPT_WINDOW_MS`].
pub fn events_to_prompt(
    events: &[CalendarEvent],
    now: i64,
    already_prompted: impl Fn(&str) -> bool,
) -> Vec<&CalendarEvent> {
    events
        .iter()
        .filter(|e| {
            e.meeting_url.is_some()
                && !is_all_day(e)
                && e.attendees.len() >= PROMPT_MIN_ATTENDEES
                && e.start_at - PROMPT_LEAD_MS <= now
                && now - e.start_at < PROMPT_WINDOW_MS
                && !already_prompted(&e.id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: i64 = MINUTE_MS;

    fn state(started: i64, end: Option<i64>, last: Option<i64>) -> ReminderState {
        ReminderState {
            started_at: started,
            scheduled_end_at: end,
            last_reminder_at: last,
            interval_minutes: 10,
        }
    }

    #[test]
    fn calendar_meeting_first_reminder_is_five_minutes_after_end() {
        let s = state(0, Some(30 * M), None);
        assert_eq!(next_reminder_at(&s), 35 * M);
        assert!(!reminder_due(35 * M - 1, &s));
        assert!(reminder_due(35 * M, &s));
    }

    #[test]
    fn later_reminders_follow_the_interval() {
        let s = state(0, Some(30 * M), Some(35 * M));
        assert_eq!(next_reminder_at(&s), 45 * M);
        let s = ReminderState {
            interval_minutes: 3,
            ..s
        };
        assert_eq!(next_reminder_at(&s), 38 * M);
        // A zero interval can't spam: it behaves as one minute.
        let s = ReminderState {
            interval_minutes: 0,
            ..s
        };
        assert_eq!(next_reminder_at(&s), 36 * M);
    }

    #[test]
    fn manual_session_starts_reminding_after_an_hour() {
        let s = state(1000, None, None);
        assert_eq!(next_reminder_at(&s), 1000 + 60 * M);
        assert!(!reminder_due(1000 + 59 * M, &s));
        let s = state(1000, None, Some(1000 + 60 * M));
        assert_eq!(next_reminder_at(&s), 1000 + 70 * M);
    }

    #[test]
    fn keep_going_snoozes_from_the_press() {
        // Reminder at 35, user presses Keep going at 37: next at 47.
        let s = state(0, Some(30 * M), Some(37 * M));
        assert!(!reminder_due(46 * M, &s));
        assert!(reminder_due(47 * M, &s));
    }

    fn person(name: &str) -> crate::meetings::types::Attendee {
        crate::meetings::types::Attendee {
            name: Some(name.into()),
            email: None,
            is_current_user: false,
        }
    }

    fn ev(id: &str, start: i64, end: i64, url: bool) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            title: id.into(),
            start_at: start,
            end_at: end,
            meeting_url: url.then(|| "https://meet.google.com/abc-defg-hij".into()),
            attendees: vec![person("Ann"), person("Ben")],
        }
    }

    #[test]
    fn manual_start_links_to_the_current_event() {
        let events = vec![
            ev("past", 0, 30 * M, true),
            ev("allday", 0, 24 * 60 * M, false),
            ev("standup", 60 * M, 75 * M, false),
            ev("sync", 55 * M, 90 * M, true),
            ev("later", 120 * M, 150 * M, true),
        ];
        // Both standup and sync are on; the one with a meeting link wins.
        assert_eq!(current_event(&events, 62 * M).unwrap().id, "sync");
        // End is exclusive.
        assert!(current_event(&events[..1], 30 * M).is_none());
        // Only the all-day event: no link.
        assert!(current_event(&events, 100 * M).is_none());
        // Among link-less events, the most recently started.
        let events = vec![ev("a", 0, 60 * M, false), ev("b", 10 * M, 40 * M, false)];
        assert_eq!(current_event(&events, 20 * M).unwrap().id, "b");
    }

    #[test]
    fn starts_link_to_an_event_until_its_grace_period_is_over() {
        let e = ev("ev", 0, 30 * M, true);
        assert!(links_to_event(&e, 29 * M));
        assert!(links_to_event(&e, 30 * M + START_LINK_GRACE_MS));
        assert!(!links_to_event(&e, 30 * M + START_LINK_GRACE_MS + 1));
        assert!(!links_to_event(&e, 30 * M + 3 * 60 * M));
    }

    #[test]
    fn prompts_fire_at_start_once_and_only_for_meetings() {
        let events = vec![
            ev("sync", 60 * M, 90 * M, true),
            ev("lunch", 60 * M, 90 * M, false),
            ev("allday", 0, 24 * 60 * M, true),
            ev("later", 70 * M, 80 * M, true),
        ];
        let none = |_: &str| false;
        let t = 60 * M - PROMPT_LEAD_MS;
        assert!(events_to_prompt(&events, t - 1, none).is_empty());
        let ids: Vec<_> = events_to_prompt(&events, t, none)
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, ["sync"]);
        assert_eq!(events_to_prompt(&events, 61 * M, none).len(), 1);
        assert!(events_to_prompt(&events, 62 * M, none).is_empty());
        assert!(events_to_prompt(&events, 60 * M, |id| id == "sync").is_empty());
    }

    #[test]
    fn a_15_second_tick_prompts_a_minute_ahead_give_or_take_ten_seconds() {
        let events = vec![ev("sync", 60 * M, 90 * M, true)];
        // Whatever the ticks' phase, the first one in the window.
        for phase in 0..15_000 {
            let first = (0..)
                .map(|n| 50 * M + phase + n * 15_000)
                .find(|t| !events_to_prompt(&events, *t, |_| false).is_empty())
                .unwrap();
            let lead = 60 * M - first;
            assert!((50_000..=70_000).contains(&lead), "{lead}");
        }
    }

    #[test]
    fn prompts_need_two_attendees() {
        let mut one = ev("one", 60 * M, 90 * M, true);
        one.attendees.truncate(1);
        let mut none = ev("none", 60 * M, 90 * M, true);
        none.attendees.clear();
        let events = [one, none];
        assert!(events_to_prompt(&events, 60 * M, |_| false).is_empty());
    }

    #[test]
    fn a_start_links_to_a_call_about_to_begin_or_just_over() {
        let call = ev("call", 60 * M, 90 * M, true);
        let block = ev("block", 60 * M, 90 * M, false);
        let events = [call, block];
        let near = |now| nearby_call_event(&events, now).map(|e| e.id.as_str());
        assert_eq!(near(60 * M - UPCOMING_LINK_MS), Some("call"));
        assert_eq!(near(60 * M - UPCOMING_LINK_MS - 1), None);
        assert_eq!(near(90 * M), Some("call"));
        assert_eq!(near(90 * M + START_LINK_GRACE_MS), Some("call"));
        assert_eq!(near(90 * M + START_LINK_GRACE_MS + 1), None);
        // During it `current_event` answers.
        assert_eq!(near(70 * M), None);
    }
}
