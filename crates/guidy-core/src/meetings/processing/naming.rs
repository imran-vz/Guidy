//! Naming speakers: picker suggestions, assigning a person, confirming an
//! automatic match. Naming stores the speaker's voiceprints under the person
//! (D11: voiceprints only for people the user names).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Result};

use super::cluster::{rank_people, KnownVoice};
use super::sidecar::{self, SegmentEmbedding};
use crate::meetings::manager::{NameSuggestion, PersonRef, SuggestionSource};
use crate::meetings::settings::MeetingSettings;
use crate::meetings::store::MeetingsStore;
use crate::meetings::types::*;

/// At most this many voiceprints are stored per person per meeting (the
/// longest segments win).
pub const MAX_PRINTS_PER_MEETING: usize = 20;

/// Every named person's voiceprints, grouped per person (sorted by id, so
/// matching is deterministic).
pub(crate) fn known_voices(store: &MeetingsStore) -> Result<Vec<KnownVoice>> {
    let mut by_person: HashMap<PersonId, Vec<Vec<f32>>> = HashMap::new();
    for vp in store.voiceprints()? {
        by_person
            .entry(vp.person_id)
            .or_default()
            .push(vp.embedding);
    }
    let mut voices: Vec<KnownVoice> = by_person
        .into_iter()
        .map(|(person_id, prints)| KnownVoice { person_id, prints })
        .collect();
    voices.sort_by_key(|v| v.person_id);
    Ok(voices)
}

fn same_text(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Picker suggestions for `speaker_id`: the linked event's attendees who
/// aren't assigned to another speaker of this meeting (and aren't the user,
/// by `Attendee::is_current_user` or one of the settings emails),
/// then every other known person, ranked by voice similarity (people without
/// voiceprints last, by name).
pub fn suggestions(
    store: &MeetingsStore,
    speaker_id: SpeakerId,
    settings: &MeetingSettings,
) -> Result<Vec<NameSuggestion>> {
    let Some(speaker) = store.get_speaker(speaker_id)? else {
        bail!("speaker {speaker_id} not found");
    };
    let voice = store.speaker_embedding(speaker_id)?;
    ranked_suggestions(
        store,
        speaker.meeting_id,
        voice.as_deref(),
        Some(speaker_id),
        settings,
    )
}

/// Picker suggestions for one segment (tagging a single line): like
/// [`suggestions`], ranked by that segment's voiceprint, and including the
/// people already named in the meeting (the line may well be one of them).
pub fn segment_suggestions(
    store: &MeetingsStore,
    meetings_dir: &Path,
    key: &[u8; 32],
    segment_id: i64,
    settings: &MeetingSettings,
) -> Result<Vec<NameSuggestion>> {
    let Some(segment) = store.get_segment(segment_id)? else {
        bail!("segment {segment_id} not found");
    };
    let voice = segment_voiceprint(meetings_dir, key, &segment);
    ranked_suggestions(store, segment.meeting_id, voice.as_deref(), None, settings)
}

/// One segment's voiceprint from the meeting's sidecar, if it has one.
fn segment_voiceprint(meetings_dir: &Path, key: &[u8; 32], segment: &Segment) -> Option<Vec<f32>> {
    match sidecar::read_embeddings(meetings_dir, segment.meeting_id, key) {
        Ok(v) => v?
            .into_iter()
            .find(|e| e.segment_id == segment.id && !e.embedding.is_empty())
            .map(|e| e.embedding),
        Err(e) => {
            log::warn!(
                "meeting {}: segment voiceprints unreadable: {e:#}",
                segment.meeting_id
            );
            None
        }
    }
}

/// Attendees, then known people ranked by similarity to `voice`. With
/// `naming: Some(speaker)`, people already given to the meeting's other
/// speakers are left out.
fn ranked_suggestions(
    store: &MeetingsStore,
    meeting_id: MeetingId,
    voice: Option<&[f32]>,
    naming: Option<SpeakerId>,
    settings: &MeetingSettings,
) -> Result<Vec<NameSuggestion>> {
    let meeting = store.get_meeting(meeting_id)?;
    let people = store.list_people()?;
    let person_by_id: HashMap<PersonId, &Person> = people.iter().map(|p| (p.id, p)).collect();
    let similarity: HashMap<PersonId, f32> = match voice {
        Some(c) if !c.is_empty() => rank_people(c, &known_voices(store)?).into_iter().collect(),
        _ => HashMap::new(),
    };

    // People already given to the meeting's other speakers.
    let assigned: Vec<&Person> = match naming {
        Some(speaker_id) => store
            .speakers(meeting_id)?
            .iter()
            .filter(|s| s.id != speaker_id)
            .filter_map(|s| s.person_id.and_then(|id| person_by_id.get(&id).copied()))
            .collect(),
        None => Vec::new(),
    };
    let is_assigned = |name: Option<&str>, email: Option<&str>| {
        assigned.iter().any(|p| match (email, p.email.as_deref()) {
            (Some(a), Some(b)) => same_text(a, b),
            _ => name.is_some_and(|n| same_text(n, &p.display_name)),
        })
    };

    let mut out = Vec::new();
    let mut listed: HashSet<PersonId> = HashSet::new();
    let attendees = meeting
        .and_then(|m| m.calendar_event)
        .map(|e| e.attendees)
        .unwrap_or_default();
    for a in attendees {
        // The Mac's user is the mic track, never one of the other speakers.
        if a.is_current_user {
            continue;
        }
        let email = a.email.as_deref().map(str::trim).filter(|e| !e.is_empty());
        let name = a.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
        if email.is_some_and(|e| settings.is_user_email(e)) {
            continue;
        }
        if is_assigned(name, email) {
            continue;
        }
        let person = match email {
            Some(e) => store.find_person_by_email(e)?,
            None => name.and_then(|n| {
                people
                    .iter()
                    .find(|p| same_text(&p.display_name, n))
                    .cloned()
            }),
        };
        let display_name = person
            .as_ref()
            .map(|p| p.display_name.clone())
            .or(name.map(str::to_string))
            .or(email.map(|e| e.split('@').next().unwrap_or(e).to_string()));
        let Some(display_name) = display_name else {
            continue;
        };
        if let Some(p) = &person {
            if !listed.insert(p.id) {
                continue;
            }
        }
        out.push(NameSuggestion {
            display_name,
            email: email
                .map(str::to_string)
                .or(person.as_ref().and_then(|p| p.email.clone())),
            person_id: person.as_ref().map(|p| p.id),
            source: SuggestionSource::Attendee,
            similarity: person.as_ref().and_then(|p| similarity.get(&p.id).copied()),
        });
    }

    let mut known: Vec<&Person> = people.iter().filter(|p| !listed.contains(&p.id)).collect();
    known.sort_by(|a, b| {
        let (sa, sb) = (similarity.get(&a.id), similarity.get(&b.id));
        match (sa, sb) {
            (Some(x), Some(y)) => y.total_cmp(x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a
                .display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase()),
        }
    });
    out.extend(known.into_iter().map(|p| NameSuggestion {
        display_name: p.display_name.clone(),
        email: p.email.clone(),
        person_id: Some(p.id),
        source: SuggestionSource::KnownPerson,
        similarity: similarity.get(&p.id).copied(),
    }));
    Ok(out)
}

/// This speaker's voiceprints, longest segments first (at most
/// [`MAX_PRINTS_PER_MEETING`]). Falls back to the cluster centroid when the
/// per-segment sidecar is missing.
pub fn speaker_prints(
    store: &MeetingsStore,
    meetings_dir: &Path,
    key: &[u8; 32],
    speaker: &MeetingSpeaker,
) -> Result<Vec<Vec<f32>>> {
    let segment_ids: HashSet<i64> = store
        .segments(speaker.meeting_id)?
        .into_iter()
        .filter(|s| s.speaker_id == Some(speaker.id))
        .map(|s| s.id)
        .collect();
    let stored: Vec<SegmentEmbedding> =
        match sidecar::read_embeddings(meetings_dir, speaker.meeting_id, key) {
            Ok(v) => v.unwrap_or_default(),
            Err(e) => {
                log::warn!(
                    "meeting {}: segment voiceprints unreadable: {e:#}",
                    speaker.meeting_id
                );
                Vec::new()
            }
        };
    let mut mine: Vec<SegmentEmbedding> = stored
        .into_iter()
        .filter(|e| segment_ids.contains(&e.segment_id) && !e.embedding.is_empty())
        .collect();
    mine.sort_by(|a, b| {
        b.duration_ms
            .cmp(&a.duration_ms)
            .then(a.segment_id.cmp(&b.segment_id))
    });
    let prints: Vec<Vec<f32>> = mine
        .into_iter()
        .take(MAX_PRINTS_PER_MEETING)
        .map(|e| e.embedding)
        .collect();
    if !prints.is_empty() {
        return Ok(prints);
    }
    Ok(store
        .speaker_embedding(speaker.id)?
        .filter(|c| !c.is_empty())
        .into_iter()
        .collect())
}

/// Store the speaker's voiceprints under `person_id`, skipping ones already
/// stored and keeping the per-meeting cap. Returns how many were added.
pub fn store_prints(
    store: &MeetingsStore,
    meetings_dir: &Path,
    key: &[u8; 32],
    speaker: &MeetingSpeaker,
    person_id: PersonId,
) -> Result<usize> {
    let prints = speaker_prints(store, meetings_dir, key, speaker)?;
    let existing: Vec<Vec<f32>> = store
        .voiceprints_for_person(person_id)?
        .into_iter()
        .filter(|v| v.source_meeting_id == Some(speaker.meeting_id))
        .map(|v| v.embedding)
        .collect();
    let mut room = MAX_PRINTS_PER_MEETING.saturating_sub(existing.len());
    let mut added = 0;
    for p in prints {
        if room == 0 {
            break;
        }
        if existing.contains(&p) {
            continue;
        }
        store.add_voiceprint(person_id, &p, Some(speaker.meeting_id))?;
        room -= 1;
        added += 1;
    }
    Ok(added)
}

/// Remove this speaker's voiceprints from everyone but `keep`: they were
/// given to someone else before (a reassignment, or two voices merged into
/// one speaker and named), and would keep matching the wrong person.
fn remove_prints_from_others(
    store: &MeetingsStore,
    meetings_dir: &Path,
    key: &[u8; 32],
    speaker: &MeetingSpeaker,
    keep: PersonId,
) -> Result<()> {
    let prints = speaker_prints(store, meetings_dir, key, speaker)?;
    for v in store.voiceprints()? {
        if v.person_id != keep
            && v.source_meeting_id == Some(speaker.meeting_id)
            && prints.contains(&v.embedding)
        {
            store.delete_voiceprint(v.id)?;
        }
    }
    Ok(())
}

/// Name a speaker (creating the person if new), store their voiceprints and
/// mark the speaker confirmed. Returns the person id.
pub fn assign(
    store: &MeetingsStore,
    meetings_dir: &Path,
    key: &[u8; 32],
    speaker_id: SpeakerId,
    person: PersonRef,
) -> Result<PersonId> {
    let Some(mut speaker) = store.get_speaker(speaker_id)? else {
        bail!("speaker {speaker_id} not found");
    };
    let person_id = resolve_person(store, person)?;
    let previous = speaker.person_id;
    if !speaker.is_user {
        remove_prints_from_others(store, meetings_dir, key, &speaker, person_id)?;
    }
    if previous != Some(person_id) {
        // The automatic match (if any) was for someone else.
        speaker.match_confidence = None;
    }
    speaker.person_id = Some(person_id);
    speaker.confirmed = true;
    store.update_speaker(&speaker)?;
    if !speaker.is_user {
        store_prints(store, meetings_dir, key, &speaker, person_id)?;
    }
    Ok(person_id)
}

/// The person a [`PersonRef`] names, creating a new one (or finding them
/// by email).
fn resolve_person(store: &MeetingsStore, person: PersonRef) -> Result<PersonId> {
    Ok(match person {
        PersonRef::Existing(id) => match store.get_person(id)? {
            Some(p) => p.id,
            None => bail!("person {id} not found"),
        },
        PersonRef::New {
            display_name,
            email,
        } => {
            let name = display_name.trim();
            if name.is_empty() {
                bail!("a name is required");
            }
            let email = email.as_deref().map(str::trim).filter(|e| !e.is_empty());
            match email
                .map(|e| store.find_person_by_email(e))
                .transpose()?
                .flatten()
            {
                Some(p) => p.id,
                None => store.insert_person(name, email)?.id,
            }
        }
    })
}

/// Say who spoke one segment (a line the diarization left unknown or got
/// wrong). It moves to that person's speaker in the meeting (created when
/// they have none yet), and its voiceprint moves too: out of the person it
/// was wrongly given to, into this one's, so the next meeting knows both
/// voices better. A speaker left without segments is removed.
pub fn assign_segment(
    store: &MeetingsStore,
    meetings_dir: &Path,
    key: &[u8; 32],
    segment_id: i64,
    person: PersonRef,
) -> Result<PersonId> {
    let Some(segment) = store.get_segment(segment_id)? else {
        bail!("segment {segment_id} not found");
    };
    let person_id = resolve_person(store, person)?;
    let meeting_id = segment.meeting_id;
    let speakers = store.speakers(meeting_id)?;
    let previous = segment
        .speaker_id
        .and_then(|id| speakers.iter().find(|s| s.id == id).cloned());
    let target = match speakers
        .iter()
        .find(|s| s.person_id == Some(person_id) && !s.is_user)
    {
        Some(s) => s.clone(),
        None => {
            let name = store
                .get_person(person_id)?
                .map(|p| p.display_name)
                .unwrap_or_default();
            store.insert_speaker(
                meeting_id,
                &MeetingSpeaker {
                    id: 0,
                    meeting_id,
                    cluster_label: name,
                    person_id: Some(person_id),
                    match_confidence: None,
                    confirmed: true,
                    is_user: false,
                },
            )?
        }
    };
    store.set_segment_speaker(segment_id, Some(target.id))?;

    let voice = segment_voiceprint(meetings_dir, key, &segment);
    if let Some(voice) = &voice {
        // Out of whoever it was wrongly given to (one print each)...
        let mut cleared = HashSet::new();
        for v in store.voiceprints()? {
            if v.person_id != person_id
                && v.source_meeting_id == Some(meeting_id)
                && &v.embedding == voice
                && cleared.insert(v.person_id)
            {
                store.delete_voiceprint(v.id)?;
            }
        }
        // ...into this person's, within the per-meeting cap.
        let mine: Vec<Vec<f32>> = store
            .voiceprints_for_person(person_id)?
            .into_iter()
            .filter(|v| v.source_meeting_id == Some(meeting_id))
            .map(|v| v.embedding)
            .collect();
        if !mine.contains(voice) && mine.len() < MAX_PRINTS_PER_MEETING {
            store.add_voiceprint(person_id, voice, Some(meeting_id))?;
        }
    }
    if let Some(old) = previous.filter(|s| s.id != target.id && !s.is_user) {
        let left = store
            .segments(meeting_id)?
            .iter()
            .any(|s| s.speaker_id == Some(old.id));
        if !left {
            store.delete_speaker(old.id)?;
        }
    }
    Ok(person_id)
}

/// Accept an uncertain automatic match ("Confirm?").
pub fn confirm(
    store: &MeetingsStore,
    meetings_dir: &Path,
    key: &[u8; 32],
    speaker_id: SpeakerId,
) -> Result<()> {
    let Some(mut speaker) = store.get_speaker(speaker_id)? else {
        bail!("speaker {speaker_id} not found");
    };
    let Some(person_id) = speaker.person_id else {
        bail!("this speaker has no suggested person to confirm");
    };
    speaker.confirmed = true;
    store.update_speaker(&speaker)?;
    if !speaker.is_user {
        store_prints(store, meetings_dir, key, &speaker, person_id)?;
    }
    Ok(())
}
