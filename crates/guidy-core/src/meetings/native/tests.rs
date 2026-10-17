use super::*;
use std::sync::{Arc, Mutex};

// ----- find_meeting_url -----------------------------------------------------

#[test]
fn finds_google_meet_with_and_without_scheme() {
    assert_eq!(
        find_meeting_url("https://meet.google.com/abc-defg-hij").as_deref(),
        Some("https://meet.google.com/abc-defg-hij")
    );
    assert_eq!(
        find_meeting_url("Join: meet.google.com/abc-defg-hij.").as_deref(),
        Some("https://meet.google.com/abc-defg-hij")
    );
    assert_eq!(
        find_meeting_url("http://meet.google.com/abc-defg-hij?authuser=0").as_deref(),
        Some("https://meet.google.com/abc-defg-hij?authuser=0")
    );
}

#[test]
fn finds_meet_link_inside_google_calendar_description_markup() {
    let notes = "Join with Google Meet: <https://meet.google.com/xyz-abcd-efg>\n\
                 Or dial: +1 555-0100 PIN: 123#\n\
                 Learn more about Meet at: https://support.google.com/a/users/answer/9282720";
    assert_eq!(
        find_meeting_url(notes).as_deref(),
        Some("https://meet.google.com/xyz-abcd-efg")
    );
    let html = r#"<a href="https://meet.google.com/xyz-abcd-efg">Join</a>"#;
    assert_eq!(
        find_meeting_url(html).as_deref(),
        Some("https://meet.google.com/xyz-abcd-efg")
    );
}

#[test]
fn finds_zoom_links_on_any_subdomain() {
    assert_eq!(
        find_meeting_url("https://us02web.zoom.us/j/81234567890?pwd=AbC123.1").as_deref(),
        Some("https://us02web.zoom.us/j/81234567890?pwd=AbC123.1")
    );
    assert_eq!(
        find_meeting_url("(https://zoom.us/j/123456789)").as_deref(),
        Some("https://zoom.us/j/123456789")
    );
    assert_eq!(
        find_meeting_url("acme.zoom.us/my/alice").as_deref(),
        Some("https://acme.zoom.us/my/alice")
    );
    assert_eq!(
        find_meeting_url("https://agency.zoomgov.com/j/1601234567").as_deref(),
        Some("https://agency.zoomgov.com/j/1601234567")
    );
}

#[test]
fn finds_teams_links() {
    let url = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_NjQ%40thread.v2/0?context=%7b%22Tid%22%3a%22abc%22%7d";
    let text = format!("Microsoft Teams meeting\nJoin on your computer: <{url}>\nMeeting ID: 123");
    assert_eq!(find_meeting_url(&text).as_deref(), Some(url));
    assert_eq!(
        find_meeting_url("https://teams.live.com/meet/9876543210").as_deref(),
        Some("https://teams.live.com/meet/9876543210")
    );
}

#[test]
fn ignores_non_meeting_links_and_lookalikes() {
    assert_eq!(find_meeting_url(""), None);
    assert_eq!(find_meeting_url("Room 4B, 2nd floor"), None);
    assert_eq!(find_meeting_url("https://zoom.us/pricing"), None);
    assert_eq!(find_meeting_url("https://zoom.us/j/"), None);
    assert_eq!(find_meeting_url("https://meet.google.com/"), None);
    assert_eq!(find_meeting_url("meet.google.com/."), None);
    assert_eq!(
        find_meeting_url("https://notmeet.google.com/abc-defg-hij"),
        None
    );
    assert_eq!(
        find_meeting_url("https://teams.microsoft.com/l/channel/x"),
        None
    );
    assert_eq!(find_meeting_url("https://example.com/?u=zoom"), None);
}

#[test]
fn returns_the_first_link_in_the_text() {
    let text = "Backup: https://zoom.us/j/111 main: https://meet.google.com/aaa-bbbb-ccc";
    assert_eq!(
        find_meeting_url(text).as_deref(),
        Some("https://zoom.us/j/111")
    );
}

#[test]
fn host_match_is_case_insensitive() {
    assert_eq!(
        find_meeting_url("HTTPS://Meet.Google.com/abc-defg-hij").as_deref(),
        Some("HTTPS://Meet.Google.com/abc-defg-hij")
    );
}

// ----- events ---------------------------------------------------------------

fn raw(title: &str) -> RawEvent {
    RawEvent {
        external_id: Some("EXT-1".into()),
        event_id: Some("EV-1".into()),
        title: title.into(),
        start_ms: 1_700_000_000_000,
        end_ms: 1_700_001_800_000,
        ..Default::default()
    }
}

#[test]
fn event_id_prefers_external_identifier_and_appends_start() {
    let e = raw("x");
    assert_eq!(event_id(&e).as_deref(), Some("EXT-1@1700000000000"));
    let e = RawEvent {
        external_id: Some("  ".into()),
        ..raw("x")
    };
    assert_eq!(event_id(&e).as_deref(), Some("EV-1@1700000000000"));
    let e = RawEvent {
        external_id: None,
        event_id: None,
        ..raw("x")
    };
    assert_eq!(event_id(&e), None);
}

#[test]
fn recurring_occurrences_get_distinct_ids() {
    let a = raw("x");
    let b = RawEvent {
        start_ms: a.start_ms + 7 * 24 * 3600 * 1000,
        ..raw("x")
    };
    assert_ne!(event_id(&a), event_id(&b));
}

#[test]
fn meeting_events_keeps_only_timed_events_with_links() {
    let with_url = RawEvent {
        url: Some("https://meet.google.com/aaa-bbbb-ccc".into()),
        ..raw(" Weekly sync ")
    };
    let in_location = RawEvent {
        external_id: Some("EXT-2".into()),
        start_ms: with_url.start_ms - 1000,
        location: Some("https://acme.zoom.us/j/123".into()),
        ..raw("Standup")
    };
    let in_notes = RawEvent {
        external_id: Some("EXT-3".into()),
        start_ms: with_url.start_ms + 1000,
        notes: Some("Join: https://teams.microsoft.com/l/meetup-join/abc".into()),
        ..raw("Review")
    };
    let all_day = RawEvent {
        external_id: Some("EXT-4".into()),
        all_day: true,
        url: Some("https://meet.google.com/aaa-bbbb-ccc".into()),
        ..raw("Offsite")
    };
    let no_link = RawEvent {
        external_id: Some("EXT-5".into()),
        location: Some("Room 1".into()),
        ..raw("Lunch")
    };

    let events = meeting_events(vec![with_url, all_day, in_notes, no_link, in_location]);
    let titles: Vec<_> = events.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["Standup", "Weekly sync", "Review"]);
    assert_eq!(
        events[0].meeting_url.as_deref(),
        Some("https://acme.zoom.us/j/123")
    );
    assert_eq!(
        events[2].meeting_url.as_deref(),
        Some("https://teams.microsoft.com/l/meetup-join/abc")
    );
}

#[test]
fn url_field_wins_over_location_and_notes() {
    let e = RawEvent {
        url: Some("https://meet.google.com/aaa-bbbb-ccc".into()),
        location: Some("https://zoom.us/j/1".into()),
        notes: Some("https://zoom.us/j/2".into()),
        ..raw("x")
    };
    let events = meeting_events(vec![e]);
    assert_eq!(
        events[0].meeting_url.as_deref(),
        Some("https://meet.google.com/aaa-bbbb-ccc")
    );
}

#[test]
fn falls_through_a_non_meeting_url_field() {
    let e = RawEvent {
        url: Some("https://example.com/agenda".into()),
        notes: Some("https://zoom.us/j/2".into()),
        ..raw("x")
    };
    assert_eq!(
        meeting_events(vec![e])[0].meeting_url.as_deref(),
        Some("https://zoom.us/j/2")
    );
}

#[test]
fn attendees_get_names_and_mailto_emails() {
    let e = RawEvent {
        url: Some("https://meet.google.com/aaa-bbbb-ccc".into()),
        attendees: vec![
            RawAttendee {
                name: Some("Alice Smith".into()),
                url: Some("mailto:alice@example.com".into()),
                is_current_user: false,
            },
            RawAttendee {
                name: Some("bob@example.com".into()),
                url: Some("MAILTO:bob%40example.com?subject=x".into()),
                is_current_user: true,
            },
            RawAttendee {
                name: Some("Conference Room".into()),
                url: Some("urn:uuid:1234".into()),
                is_current_user: false,
            },
            RawAttendee {
                name: Some("  ".into()),
                url: Some("https://example.com".into()),
                is_current_user: false,
            },
        ],
        ..raw("x")
    };
    let events = meeting_events(vec![e]);
    assert_eq!(
        events[0].attendees,
        vec![
            Attendee {
                name: Some("Alice Smith".into()),
                email: Some("alice@example.com".into())
            },
            Attendee {
                name: None,
                email: Some("bob@example.com".into())
            },
            Attendee {
                name: Some("Conference Room".into()),
                email: None
            },
        ]
    );
}

#[test]
fn email_from_participant_url_rejects_garbage() {
    assert_eq!(email_from_participant_url("mailto:"), None);
    assert_eq!(email_from_participant_url("mailto:not an email"), None);
    assert_eq!(email_from_participant_url("http://a@b.c"), None);
    assert_eq!(email_from_participant_url("mail"), None);
    assert_eq!(
        email_from_participant_url("mailto:%E2%82%AC@x.io").as_deref(),
        Some("\u{20ac}@x.io")
    );
    assert_eq!(
        email_from_participant_url("mailto:é%@x.io").as_deref(),
        Some("é%@x.io")
    );
}

#[test]
fn raw_events_parse_from_bridge_json() {
    let json = r#"[{"external_id":"E","event_id":null,"title":"T","start_ms":1,"end_ms":2,
        "all_day":false,"url":null,"location":"meet.google.com/abc-defg-hij","notes":null,
        "attendees":[{"name":null,"url":"mailto:a@b.co","is_current_user":false}]}]"#;
    let raw: Vec<RawEvent> = serde_json::from_str(json).unwrap();
    let events = meeting_events(raw);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "E@1");
    assert_eq!(events[0].attendees[0].email.as_deref(), Some("a@b.co"));
}

// ----- capture hand-off -----------------------------------------------------

#[test]
fn deliver_routes_messages_in_order_and_ends_when_senders_drop() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
    let callbacks = CaptureCallbacks::new(
        move |s, r| l1.lock().unwrap().push(format!("mic {} {r}", s.len())),
        move |s, r| l2.lock().unwrap().push(format!("sys {} {r}", s.len())),
    )
    .with_events(move |e| {
        l3.lock()
            .unwrap()
            .push(format!("event {:?} {}", e.track, e.message))
    });

    let (tx, rx) = mpsc::channel();
    let thread = std::thread::spawn(move || deliver(rx, callbacks));
    tx.send(CaptureMsg::Audio(Track::Mic, vec![0.0; 160], 16_000))
        .unwrap();
    tx.send(CaptureMsg::Audio(Track::System, vec![0.0; 320], 16_000))
        .unwrap();
    tx.send(CaptureMsg::Event(CaptureEvent {
        track: Track::System,
        message: "error: gone".into(),
    }))
    .unwrap();
    drop(tx);
    thread.join().unwrap();

    assert_eq!(
        *log.lock().unwrap(),
        ["mic 160 16000", "sys 320 16000", "event System error: gone"]
    );
}

#[test]
fn capture_event_error_flag() {
    let e = CaptureEvent {
        track: Track::Mic,
        message: "error: microphone restart failed".into(),
    };
    assert!(e.is_error());
    let e = CaptureEvent {
        track: Track::Mic,
        message: "microphone restarted".into(),
    };
    assert!(!e.is_error());
}

#[test]
fn starting_with_no_streams_fails() {
    let r = start_capture_streams(
        CaptureStreams {
            mic: false,
            system: false,
        },
        CaptureCallbacks::new(|_, _| {}, |_, _| {}),
    );
    assert!(r.is_err());
}

#[test]
fn auth_status_from_raw() {
    assert_eq!(AuthStatus::from_raw(0), AuthStatus::NotDetermined);
    assert_eq!(AuthStatus::from_raw(1), AuthStatus::Restricted);
    assert_eq!(AuthStatus::from_raw(2), AuthStatus::Denied);
    assert_eq!(AuthStatus::from_raw(3), AuthStatus::Granted);
    assert_eq!(AuthStatus::from_raw(4), AuthStatus::WriteOnly);
    assert_eq!(AuthStatus::from_raw(5), AuthStatus::Unknown);
    assert_eq!(AuthStatus::from_raw(6), AuthStatus::Unavailable);
    assert_eq!(AuthStatus::from_raw(-1), AuthStatus::Unknown);
    assert!(AuthStatus::Granted.is_granted());
    assert!(!AuthStatus::WriteOnly.is_granted());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn other_platforms_are_unsupported() {
    assert!(!is_supported());
    assert!(events_between(0, 1).is_err());
    assert!(start_capture(CaptureCallbacks::new(|_, _| {}, |_, _| {})).is_err());
    assert!(!post_notification("id", "t", "b", &[]));
    assert_eq!(calendar_authorization(), AuthStatus::Unavailable);
}

// ----- bridge smoke tests (no prompts, no hardware) --------------------------

#[cfg(target_os = "macos")]
#[test]
fn bridge_links_and_answers_status_queries() {
    // Reading statuses never prompts.
    let _ = is_supported();
    let _ = calendar_authorization();
    let _ = system_audio_status();
    // `cargo test` binaries are unbundled: notifications must degrade, not crash.
    assert!(!notifications_available());
    assert_eq!(notification_status(), AuthStatus::Unavailable);
    assert!(!post_notification(
        "t",
        "title",
        "body",
        &[("start", "Start")]
    ));
    remove_notification("t");
    set_notification_handler(|_, _| {});
}

// ----- hardware integration (run by hand) -----------------------------------
//
//   cargo test -p guidy-core meetings::native::tests::hw -- --ignored --nocapture
//
// Grants are attributed to the terminal app running the test.

#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs calendar permission; run by hand"]
fn hw_lists_todays_calendar_events() {
    println!("supported: {}", is_supported());
    println!("calendar status: {:?}", calendar_authorization());
    if !calendar_authorization().is_granted() {
        println!("requesting calendar access: {}", request_calendar_access());
    }
    let now = chrono::Local::now();
    let start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp_millis();
    let end = start + 24 * 3600 * 1000;
    let events = events_between(start, end).expect("events_between");
    println!("{} meeting(s) today", events.len());
    for e in &events {
        println!(
            "- {} [{} .. {}] {:?} attendees={}",
            e.title,
            e.start_at,
            e.end_at,
            e.meeting_url,
            e.attendees.len()
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs microphone + system audio permission; run by hand"]
fn hw_captures_two_seconds() {
    assert!(is_supported(), "needs macOS 14.2+");
    println!("system audio status: {:?}", system_audio_status());

    #[derive(Default)]
    struct Stats {
        frames: usize,
        peak: f32,
        rates: std::collections::BTreeSet<u32>,
    }
    let mic = Arc::new(Mutex::new(Stats::default()));
    let sys = Arc::new(Mutex::new(Stats::default()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let (m, s, ev) = (mic.clone(), sys.clone(), events.clone());
    let record = |stats: &Mutex<Stats>, samples: &[f32], rate: u32| {
        let mut st = stats.lock().unwrap();
        st.frames += samples.len();
        st.peak = samples.iter().fold(st.peak, |p, x| p.max(x.abs()));
        st.rates.insert(rate);
    };
    let callbacks =
        CaptureCallbacks::new(move |x, r| record(&m, x, r), move |x, r| record(&s, x, r))
            .with_events(move |e| ev.lock().unwrap().push(e));

    // Play something so the system track has signal (best effort).
    let player = std::process::Command::new("afplay")
        .args(["-t", "2", "/System/Library/Sounds/Submarine.aiff"])
        .spawn()
        .ok();
    // GUIDY_HW_STREAMS=mic|system|both (default both).
    let which = std::env::var("GUIDY_HW_STREAMS").unwrap_or_default();
    let streams = CaptureStreams {
        mic: which != "system",
        system: which != "mic",
    };
    let mut handle = start_capture_streams(streams, callbacks).expect("start_capture");
    std::thread::sleep(std::time::Duration::from_secs(2));
    handle.stop();
    assert!(!handle.is_running());
    if let Some(mut p) = player {
        let _ = p.wait();
    }

    let (mic, sys) = (mic.lock().unwrap(), sys.lock().unwrap());
    println!(
        "mic: {} frames, peak {:.4}, rates {:?}",
        mic.frames, mic.peak, mic.rates
    );
    println!(
        "system: {} frames, peak {:.4}, rates {:?}",
        sys.frames, sys.peak, sys.rates
    );
    println!("events: {:?}", events.lock().unwrap());
    // ~2 s at 16 kHz each (allow start-up latency).
    if streams.mic {
        assert!(mic.frames > 16_000, "mic delivered too little audio");
    }
    if streams.system {
        assert!(
            sys.frames > 16_000,
            "system audio delivered too little audio"
        );
    }
    assert!(mic
        .rates
        .iter()
        .chain(sys.rates.iter())
        .all(|r| *r == CAPTURE_SAMPLE_RATE));
}
