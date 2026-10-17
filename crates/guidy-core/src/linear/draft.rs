//! Pure ticket drafting: turn an action item into a [`TicketDraft`].

use chrono::{NaiveDate, TimeZone};

use super::{Team, User};
use crate::meetings::types::{ActionItem, Attendee, Meeting, Person, TicketDraft};

/// Longest generated title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;

/// Everything drafting needs besides the item itself.
#[derive(Debug, Clone, Default)]
pub struct DraftContext<'a> {
    pub meeting: Option<&'a Meeting>,
    pub people: &'a [Person],
    pub attendees: &'a [Attendee],
    pub linear_users: &'a [User],
    /// The authenticated Linear user (the Guidy user).
    pub viewer_id: Option<&'a str>,
    pub teams: &'a [Team],
    pub default_team_id: Option<&'a str>,
    pub default_project_id: Option<&'a str>,
}

/// Draft a ticket for `item`.
pub fn draft_ticket(item: &ActionItem, ctx: &DraftContext) -> TicketDraft {
    let due_date = item.due.as_deref().and_then(parse_iso_date);
    let (team_id, project_id) = pick_team_project(ctx);
    TicketDraft {
        title: ticket_title(&item.text),
        description: ticket_description(item, ctx.meeting),
        team_id,
        project_id,
        priority: None,
        label_ids: Vec::new(),
        assignee_id: pick_assignee(item, ctx),
        due_date: due_date.map(|d| d.format("%Y-%m-%d").to_string()),
    }
}

/// The remembered team (if it still exists, or if teams aren't known) and
/// project; otherwise the first team and no project.
pub fn pick_team_project(ctx: &DraftContext) -> (Option<String>, Option<String>) {
    let remembered = ctx
        .default_team_id
        .filter(|id| !id.is_empty())
        .filter(|id| ctx.teams.is_empty() || ctx.teams.iter().any(|t| t.id == *id));
    match remembered {
        Some(team) => (
            Some(team.to_string()),
            ctx.default_project_id
                .filter(|p| !p.is_empty())
                .map(str::to_string),
        ),
        None => (ctx.teams.first().map(|t| t.id.clone()), None),
    }
}

/// First line of the item, whitespace-collapsed, cut at a word boundary.
pub fn ticket_title(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_TITLE_CHARS {
        return collapsed;
    }
    let cut: String = collapsed.chars().take(MAX_TITLE_CHARS - 1).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > MAX_TITLE_CHARS / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([' ', ',', ';', ':', '.', '-']))
}

/// `[mm:ss]`, or `[h:mm:ss]` past an hour.
pub fn format_timestamp(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("[{h}:{m:02}:{s:02}]")
    } else {
        format!("[{m:02}:{s:02}]")
    }
}

/// `guidy://meetings/<id>`, with `?t=<ms>` when the moment is known.
pub fn meeting_link(meeting_id: i64, t_ms: Option<i64>) -> String {
    match t_ms {
        Some(t) => format!("guidy://meetings/{meeting_id}?t={}", t.max(0)),
        None => format!("guidy://meetings/{meeting_id}"),
    }
}

/// Parse an ISO date (`YYYY-MM-DD`, optionally followed by a time).
pub fn parse_iso_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    let head = s.get(..10)?;
    let date = NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()?;
    match s[10..].chars().next() {
        None | Some('T') | Some(' ') => Some(date),
        _ => None,
    }
}

fn format_meeting_date(unix_ms: i64) -> Option<String> {
    chrono::Local
        .timestamp_millis_opt(unix_ms)
        .single()
        .map(|d| d.format("%b %-d, %Y").to_string())
}

/// Markdown body: quote with timestamp, owner, due date, meeting link.
pub fn ticket_description(item: &ActionItem, meeting: Option<&Meeting>) -> String {
    let mut out = String::new();
    let quote = item.source_quote.trim();
    if !quote.is_empty() {
        let ts = item
            .source_t_ms
            .map(|t| format!("{} ", format_timestamp(t)))
            .unwrap_or_default();
        for (i, line) in quote.lines().enumerate() {
            let prefix = if i == 0 { ts.as_str() } else { "" };
            out.push_str(&format!("> {prefix}{}\n", line.trim()));
        }
        out.push('\n');
    }
    let owner = if item.owner_is_user {
        Some("me".to_string())
    } else {
        item.owner_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    if let Some(owner) = owner {
        out.push_str(&format!("**Owner:** {owner}  \n"));
    }
    if let Some(due) = item.due.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        // Unparsed due dates ("next Friday") only live here; parsed ones are
        // also set as the issue's `dueDate`.
        out.push_str(&format!("**Due:** {due}  \n"));
    }
    let title = meeting
        .map(|m| m.title.trim())
        .filter(|t| !t.is_empty())
        .unwrap_or("meeting");
    let date = meeting
        .and_then(|m| format_meeting_date(m.started_at))
        .map(|d| format!(" ({d})"))
        .unwrap_or_default();
    out.push_str(&format!(
        "\nFrom **{title}**{date}: [open in Guidy]({})\n",
        meeting_link(item.meeting_id, item.source_t_ms)
    ));
    out.trim_start().to_string()
}

/// The Linear user to assign: the viewer when the owner is the user or
/// nobody; the owner's Linear account matched by email otherwise; `None`
/// (user picks) when the owner can't be matched.
pub fn pick_assignee(item: &ActionItem, ctx: &DraftContext) -> Option<String> {
    let owner_name = item
        .owner_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !is_self_reference(s));
    if item.owner_is_user || (item.owner_person_id.is_none() && owner_name.is_none()) {
        return ctx.viewer_id.map(str::to_string);
    }
    let email = owner_email(item, owner_name, ctx)?;
    match_user_by_email(ctx.linear_users, &email).map(|u| u.id.clone())
}

fn is_self_reference(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "me" | "i" | "myself" | "i'll")
}

/// The owner's email: from their People entry, else the calendar attendee
/// with the same name (case-insensitive).
fn owner_email(item: &ActionItem, owner_name: Option<&str>, ctx: &DraftContext) -> Option<String> {
    let person = item
        .owner_person_id
        .and_then(|id| ctx.people.iter().find(|p| p.id == id))
        .or_else(|| {
            let name = owner_name?;
            ctx.people
                .iter()
                .find(|p| p.display_name.trim().eq_ignore_ascii_case(name))
        });
    if let Some(email) = person.and_then(|p| non_empty(p.email.as_deref())) {
        return Some(email.to_string());
    }
    let names: Vec<&str> = owner_name
        .into_iter()
        .chain(person.map(|p| p.display_name.trim()))
        .collect();
    ctx.attendees
        .iter()
        .find(|a| {
            a.name
                .as_deref()
                .map(str::trim)
                .is_some_and(|n| names.iter().any(|x| x.eq_ignore_ascii_case(n)))
        })
        .and_then(|a| non_empty(a.email.as_deref()))
        .map(str::to_string)
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// Case-insensitive email match.
pub fn match_user_by_email<'u>(users: &'u [User], email: &str) -> Option<&'u User> {
    let email = email.trim();
    if email.is_empty() {
        return None;
    }
    users.iter().find(|u| {
        u.email
            .as_deref()
            .is_some_and(|e| e.trim().eq_ignore_ascii_case(email))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::types::{MeetingStatus, TicketState};

    fn item() -> ActionItem {
        ActionItem {
            id: 1,
            meeting_id: 42,
            text: "Send the Q3 budget to finance".into(),
            owner_person_id: None,
            owner_name: None,
            owner_is_user: false,
            due: None,
            source_quote: "I'll send the budget over by Friday".into(),
            source_t_ms: Some(83_500),
            ticket: TicketDraft::default(),
            ticket_state: TicketState::Draft,
            linear_issue_id: None,
            linear_issue_identifier: None,
            linear_issue_url: None,
        }
    }

    fn meeting() -> Meeting {
        Meeting {
            id: 42,
            title: "Weekly sync".into(),
            calendar_event_id: None,
            calendar_event: None,
            // 2026-06-15T12:00:00Z: same calendar day in every timezone
            // within +-11h.
            started_at: 1_781_524_800_000,
            ended_at: None,
            mic_audio_path: None,
            system_audio_path: None,
            status: MeetingStatus::Processed,
            error: None,
            processing_stage: None,
        }
    }

    fn user(id: &str, email: &str) -> User {
        User {
            id: id.into(),
            name: id.into(),
            display_name: id.into(),
            email: Some(email.into()),
        }
    }

    fn person(id: i64, name: &str, email: Option<&str>) -> Person {
        Person {
            id,
            display_name: name.into(),
            email: email.map(Into::into),
            created_at: 0,
            voiceprint_count: 0,
        }
    }

    fn team(id: &str) -> Team {
        Team {
            id: id.into(),
            key: id.to_uppercase(),
            name: id.into(),
        }
    }

    #[test]
    fn title_is_trimmed_and_shortened() {
        assert_eq!(ticket_title("  Fix   the bug \nmore"), "Fix the bug");
        let long = "word ".repeat(60);
        let t = ticket_title(&long);
        assert!(t.chars().count() <= MAX_TITLE_CHARS);
        assert!(t.ends_with("word…"));
    }

    #[test]
    fn timestamps_and_links() {
        assert_eq!(format_timestamp(83_500), "[01:23]");
        assert_eq!(format_timestamp(3_723_000), "[1:02:03]");
        assert_eq!(
            meeting_link(42, Some(83_500)),
            "guidy://meetings/42?t=83500"
        );
        assert_eq!(meeting_link(42, None), "guidy://meetings/42");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(
            parse_iso_date("2026-10-20"),
            NaiveDate::from_ymd_opt(2026, 10, 20)
        );
        assert!(parse_iso_date("2026-10-20T09:00:00Z").is_some());
        assert!(parse_iso_date("next Friday").is_none());
        assert!(parse_iso_date("2026-13-01").is_none());
        assert!(parse_iso_date("2026-10-201").is_none());
    }

    #[test]
    fn description_has_quote_owner_due_and_link() {
        let mut it = item();
        it.owner_name = Some("Dana".into());
        it.due = Some("2026-10-20".into());
        let m = meeting();
        let d = ticket_description(&it, Some(&m));
        assert!(
            d.starts_with("> [01:23] I'll send the budget over by Friday\n"),
            "{d}"
        );
        assert!(d.contains("**Owner:** Dana"));
        assert!(d.contains("**Due:** 2026-10-20"));
        assert!(d.contains("**Weekly sync** (Jun 15, 2026)"), "{d}");
        assert!(d.contains("(guidy://meetings/42?t=83500)"));
    }

    #[test]
    fn unparsed_due_stays_in_description_only() {
        let mut it = item();
        it.due = Some("next Friday".into());
        let d = draft_ticket(&it, &DraftContext::default());
        assert_eq!(d.due_date, None);
        assert!(d.description.contains("**Due:** next Friday"));
        it.due = Some("2026-10-20".into());
        let d = draft_ticket(&it, &DraftContext::default());
        assert_eq!(d.due_date.as_deref(), Some("2026-10-20"));
    }

    #[test]
    fn assignee_defaults_to_viewer() {
        let users = [user("u-me", "me@x.com")];
        let ctx = DraftContext {
            linear_users: &users,
            viewer_id: Some("u-me"),
            ..Default::default()
        };
        // No owner.
        assert_eq!(pick_assignee(&item(), &ctx).as_deref(), Some("u-me"));
        // Owner is the user.
        let mut it = item();
        it.owner_is_user = true;
        it.owner_name = Some("Imran".into());
        assert_eq!(pick_assignee(&it, &ctx).as_deref(), Some("u-me"));
        // "me" as a name.
        let mut it = item();
        it.owner_name = Some("Me".into());
        assert_eq!(pick_assignee(&it, &ctx).as_deref(), Some("u-me"));
    }

    #[test]
    fn assignee_matched_by_person_email_case_insensitively() {
        let users = [user("u-me", "me@x.com"), user("u-dana", "Dana@Example.com")];
        let people = [person(7, "Dana Scully", Some("dana@example.COM"))];
        let ctx = DraftContext {
            people: &people,
            linear_users: &users,
            viewer_id: Some("u-me"),
            ..Default::default()
        };
        let mut it = item();
        it.owner_person_id = Some(7);
        it.owner_name = Some("Dana".into());
        assert_eq!(pick_assignee(&it, &ctx).as_deref(), Some("u-dana"));
        // By name when there's no person id.
        let mut it = item();
        it.owner_name = Some("dana scully".into());
        assert_eq!(pick_assignee(&it, &ctx).as_deref(), Some("u-dana"));
    }

    #[test]
    fn assignee_matched_through_calendar_attendee() {
        let users = [user("u-fox", "fox@fbi.gov")];
        let people = [person(3, "Fox Mulder", None)];
        let attendees = [Attendee {
            name: Some("Fox Mulder".into()),
            email: Some("FOX@fbi.gov".into()),
        }];
        let ctx = DraftContext {
            people: &people,
            attendees: &attendees,
            linear_users: &users,
            viewer_id: Some("u-me"),
            ..Default::default()
        };
        let mut it = item();
        it.owner_person_id = Some(3);
        assert_eq!(pick_assignee(&it, &ctx).as_deref(), Some("u-fox"));
    }

    #[test]
    fn unmatched_owner_is_left_for_the_user() {
        let users = [user("u-me", "me@x.com")];
        let ctx = DraftContext {
            linear_users: &users,
            viewer_id: Some("u-me"),
            ..Default::default()
        };
        let mut it = item();
        it.owner_name = Some("Walter Skinner".into());
        assert_eq!(pick_assignee(&it, &ctx), None);
    }

    #[test]
    fn team_and_project_defaults() {
        let teams = [team("t1"), team("t2")];
        let ctx = DraftContext {
            teams: &teams,
            ..Default::default()
        };
        assert_eq!(pick_team_project(&ctx), (Some("t1".into()), None));

        let ctx = DraftContext {
            teams: &teams,
            default_team_id: Some("t2"),
            default_project_id: Some("p9"),
            ..Default::default()
        };
        assert_eq!(
            pick_team_project(&ctx),
            (Some("t2".into()), Some("p9".into()))
        );

        // A remembered team that no longer exists falls back to the first.
        let ctx = DraftContext {
            teams: &teams,
            default_team_id: Some("gone"),
            default_project_id: Some("p9"),
            ..Default::default()
        };
        assert_eq!(pick_team_project(&ctx), (Some("t1".into()), None));
    }

    #[test]
    fn full_draft() {
        let teams = [team("t1")];
        let users = [user("u-me", "me@x.com")];
        let m = meeting();
        let ctx = DraftContext {
            meeting: Some(&m),
            teams: &teams,
            linear_users: &users,
            viewer_id: Some("u-me"),
            ..Default::default()
        };
        let d = draft_ticket(&item(), &ctx);
        assert_eq!(d.title, "Send the Q3 budget to finance");
        assert_eq!(d.team_id.as_deref(), Some("t1"));
        assert_eq!(d.assignee_id.as_deref(), Some("u-me"));
        assert!(d.description.contains("guidy://meetings/42?t=83500"));
    }
}
