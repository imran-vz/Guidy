//! Search across meetings: titles, descriptions, summaries and transcripts.
//!
//! Backed by the FTS5 indexes in `meetings.db` (store migration 12), so a
//! query is a few index lookups, not a scan of every transcript:
//!
//! 1. The query is split into terms the way the index tokenizer splits text
//!    (letters and digits; `"quoted words"` stay a phrase). Each term also
//!    matches as a prefix, so results come as you type, and an exact word
//!    scores above a longer word it is the start of.
//! 2. A term the index doesn't contain (not even as a prefix) is checked for
//!    a typo against the index vocabulary (edit distance 1, or 2 for long
//!    words); close words are searched instead and reported as corrections.
//! 3. A meeting matches when it has every term somewhere: in its title, its
//!    summary or any transcript line (not necessarily the same one). When no
//!    meeting has them all, meetings with the most terms are shown instead
//!    ([`SearchResults::partial`]).
//! 4. Meetings are ranked by BM25 relevance, weighted by where the words
//!    were found (title > description > summary > transcript), with
//!    diminishing returns for many transcript lines and a small nudge for
//!    recent meetings.
//! 5. Each hit comes with highlighted snippets: the title, the best summary
//!    passage and the best transcript lines (with their time, to jump to).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::meetings::types::MeetingId;

/// Where a snippet comes from.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MatchField {
    Title,
    Description,
    Summary,
    Transcript,
}

/// A piece of matched text. `highlights` are byte ranges of `text` that
/// matched the query.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Snippet {
    pub field: MatchField,
    pub text: String,
    pub highlights: Vec<Range<usize>>,
    /// Transcript lines: the segment, and its time from the recording start.
    pub segment_id: Option<i64>,
    pub t_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub meeting_id: MeetingId,
    pub score: f64,
    /// The title with its matches highlighted, when it matched.
    pub title: Option<Snippet>,
    /// The best other passages, most relevant first (at most
    /// [`MAX_SNIPPETS`]).
    pub snippets: Vec<Snippet>,
    /// How many transcript lines matched.
    pub transcript_hits: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SearchResults {
    /// Best first.
    pub hits: Vec<SearchHit>,
    /// Query words that weren't found and the word searched instead.
    pub corrections: Vec<(String, String)>,
    /// No meeting had every term: these have some of them.
    pub partial: bool,
}

/// Snippets per hit, besides the title.
pub const MAX_SNIPPETS: usize = 2;
/// Terms beyond this are ignored.
const MAX_TERMS: usize = 8;
/// Close words searched for a misspelled term.
const MAX_CORRECTIONS: usize = 3;
/// Misspelled terms corrected per query (each scans part of the
/// vocabulary); later ones are searched as typed.
const MAX_CORRECTED_TERMS: usize = 2;

/// Highlight markers handed to FTS5's `highlight()` / `snippet()`: control
/// characters that never occur in stored text.
const MARK_OPEN: char = '\u{2}';
const MARK_CLOSE: char = '\u{3}';

// Field weights: a word in the title says more about the meeting than one
// in a long transcript.
const W_MEETING: f64 = 3.0;
const W_SUMMARY: f64 = 1.5;
const W_TRANSCRIPT: f64 = 1.0;

/// One query term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTerm {
    /// Lowercased tokens; one for a word, several for a quoted phrase.
    pub tokens: Vec<String>,
    pub phrase: bool,
}

impl QueryTerm {
    fn word(&self) -> &str {
        &self.tokens[0]
    }
}

/// Whether `c` is part of a word for the index (FTS5 `unicode61`: letters,
/// numbers and private-use characters).
fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || ('\u{E000}'..='\u{F8FF}').contains(&c)
}

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !is_token_char(c))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// Split a query into terms: `"quoted words"` are a phrase, the rest are
/// words. Duplicates go; single letters go when there is anything longer.
pub fn parse_query(query: &str) -> Vec<QueryTerm> {
    let mut terms: Vec<QueryTerm> = Vec::new();
    for (i, part) in query.split('"').enumerate() {
        // Odd parts were inside quotes (an unclosed quote runs to the end).
        if i % 2 == 1 {
            let t = tokens(part);
            match t.len() {
                0 => {}
                1 => terms.push(QueryTerm {
                    tokens: t,
                    phrase: false,
                }),
                _ => terms.push(QueryTerm {
                    tokens: t,
                    phrase: true,
                }),
            }
        } else {
            terms.extend(tokens(part).into_iter().map(|t| QueryTerm {
                tokens: vec![t],
                phrase: false,
            }));
        }
    }
    let mut seen = HashSet::new();
    terms.retain(|t| seen.insert(t.tokens.clone()));
    let has_long = terms
        .iter()
        .any(|t| t.phrase || t.word().chars().count() > 1);
    if has_long {
        terms.retain(|t| t.phrase || t.word().chars().count() > 1);
    }
    terms.truncate(MAX_TERMS);
    terms
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The FTS5 expression for one term: the exact word or a word it starts,
/// or one of `alternatives` (spelling corrections). The exact word matches
/// both, so it ranks higher.
fn term_expr(term: &QueryTerm, alternatives: &[String]) -> String {
    if term.phrase {
        return quote(&term.tokens.join(" "));
    }
    let w = quote(term.word());
    let mut parts = vec![w.clone(), format!("{w}*")];
    parts.extend(alternatives.iter().map(|a| quote(a)));
    format!("({})", parts.join(" OR "))
}

/// Text with [`MARK_OPEN`]/[`MARK_CLOSE`] markers to plain text and the
/// byte ranges between them.
pub fn parse_marked(s: &str) -> (String, Vec<Range<usize>>) {
    let mut text = String::with_capacity(s.len());
    let mut ranges = Vec::new();
    let mut open: Option<usize> = None;
    for c in s.chars() {
        match c {
            MARK_OPEN => open = Some(text.len()),
            MARK_CLOSE => {
                if let Some(start) = open.take() {
                    if start < text.len() {
                        ranges.push(start..text.len());
                    }
                }
            }
            c => text.push(c),
        }
    }
    (text, ranges)
}

/// A summary snippet can come from the JSON arrays of decisions or topics:
/// show them as a list.
fn tidy_json_list(s: &str) -> String {
    let s = s.replace("\",\"", " · ").replace("\\\"", "\"");
    let s = s.trim();
    let s = s.strip_prefix("[\"").unwrap_or(s);
    let s = s.strip_suffix("\"]").unwrap_or(s);
    s.replace("[\"", "").replace("\"]", "").replace("[]", "")
}

fn snippet_of(field: MatchField, marked: &str) -> Option<Snippet> {
    let marked = if field == MatchField::Summary {
        tidy_json_list(marked)
    } else {
        marked.to_string()
    };
    let (text, highlights) = parse_marked(&marked);
    (!highlights.is_empty()).then(|| Snippet {
        field,
        text: text.trim().to_string(),
        highlights: shift_trimmed(&text, highlights),
        segment_id: None,
        t_ms: None,
    })
}

/// Ranges into `text` moved to `text.trim()`.
fn shift_trimmed(text: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let lead = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    ranges
        .into_iter()
        .filter_map(|r| {
            let (s, e) = (r.start.max(lead), r.end.min(end));
            (s < e).then(|| s - lead..e - lead)
        })
        .collect()
}

/// Optimal-string-alignment distance (Levenshtein plus swapping two
/// neighbours), or `None` once it exceeds `max`.
pub fn edit_distance(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let w = b.len() + 1;
    let mut prev2 = vec![0usize; w];
    let mut prev: Vec<usize> = (0..w).collect();
    let mut cur = vec![0usize; w];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            cur[j] = d;
            row_min = row_min.min(d);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    Some(prev[b.len()]).filter(|d| *d <= max)
}

/// Typos allowed in a word of `len` characters.
fn max_typos(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    }
}

const VOCABS: [&str; 3] = ["meetings_vocab", "summaries_vocab", "segments_vocab"];

/// Whether any indexed word starts with `word`. Asked of the index itself,
/// which folds case and accents the way it folded the text.
fn in_vocabulary(conn: &Connection, word: &str) -> Result<bool> {
    let expr = format!("{}*", quote(word));
    for table in ["meetings_fts", "summaries_fts", "segments_fts"] {
        let found = conn
            .prepare_cached(&format!(
                "SELECT 1 FROM {table} WHERE {table} MATCH ?1 LIMIT 1"
            ))?
            .exists([&expr])?;
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Indexed words within typo distance of `word`, closest and most common
/// first. A longer word counts when its start is that close (still typing).
///
/// Only words with the same first letter are looked at (a range of the
/// sorted vocabulary, not all of it): a search holds the database while it
/// runs, and a first letter is rarely the one mistyped.
fn corrections_for(conn: &Connection, word: &str) -> Result<Vec<String>> {
    let len = word.chars().count();
    let max = max_typos(len);
    let Some(first) = word.chars().next().filter(|_| max > 0) else {
        return Ok(Vec::new());
    };
    let (from, to) = (first.to_string(), format!("{first}\u{10FFFF}"));
    let shortest = len.saturating_sub(max) as i64;
    let mut best: HashMap<String, (usize, i64)> = HashMap::new();
    for v in VOCABS {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT term, doc FROM {v} WHERE term >= ?1 AND term < ?2 AND length(term) >= ?3"
        ))?;
        let mut rows = stmt.query(params![from, to, shortest])?;
        while let Some(row) = rows.next()? {
            let term: String = row.get(0)?;
            let tlen = term.chars().count();
            let whole = edit_distance(word, &term, max);
            let start = (tlen > len)
                .then(|| {
                    let head: String = term.chars().take(len).collect();
                    // A prefix match costs one more: a whole word wins.
                    edit_distance(word, &head, max)
                        .map(|d| d + 1)
                        .filter(|d| *d <= max)
                })
                .flatten();
            let Some(d) = whole.into_iter().chain(start).min() else {
                continue;
            };
            let doc: i64 = row.get(1)?;
            let e = best.entry(term).or_insert((d, 0));
            e.0 = e.0.min(d);
            e.1 += doc;
        }
    }
    let mut found: Vec<(String, (usize, i64))> = best.into_iter().collect();
    found.sort_by(|a, b| {
        a.1 .0
            .cmp(&b.1 .0)
            .then(b.1 .1.cmp(&a.1 .1))
            .then(a.0.cmp(&b.0))
    });
    Ok(found
        .into_iter()
        .take(MAX_CORRECTIONS)
        .map(|(t, _)| t)
        .collect())
}

/// Meetings that contain `expr` anywhere.
fn meetings_matching(conn: &Connection, expr: &str) -> Result<HashSet<MeetingId>> {
    let mut stmt = conn.prepare_cached(
        "SELECT rowid FROM meetings_fts WHERE meetings_fts MATCH ?1
         UNION SELECT rowid FROM summaries_fts WHERE summaries_fts MATCH ?1
         UNION SELECT g.meeting_id FROM segments_fts f
            JOIN meeting_segments g ON g.id = f.rowid
            WHERE segments_fts MATCH ?1",
    )?;
    let rows = stmt.query_map([expr], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Default)]
struct Scored {
    meeting: f64,
    summary: f64,
    /// (score, segment id, t0), best first once sorted.
    lines: Vec<(f64, i64, i64)>,
    title: Option<Snippet>,
    description: Option<Snippet>,
    summary_snippet: Option<Snippet>,
}

impl Scored {
    fn score(&self) -> f64 {
        let mut lines: Vec<f64> = self.lines.iter().map(|l| l.0).collect();
        lines.sort_by(|a, b| b.total_cmp(a));
        let top: f64 = lines
            .iter()
            .take(3)
            .zip([1.0, 0.5, 0.25])
            .map(|(s, w)| s * w)
            .sum();
        let transcript = top + 0.1 * (1.0 + lines.len() as f64).ln();
        // BM25 says little in a small index (a word in half the titles is
        // worth ~0), so where a word was found counts on its own too.
        let found = 4.0 * f64::from(u8::from(self.title.is_some()))
            + 1.5 * f64::from(u8::from(self.description.is_some()))
            + f64::from(u8::from(self.summary_snippet.is_some()));
        found + W_MEETING * self.meeting + W_SUMMARY * self.summary + W_TRANSCRIPT * transcript
    }
}

/// Search meetings for `query`; at most `limit` hits. An empty query (or
/// one with no words) finds nothing.
pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<SearchResults> {
    let terms = parse_query(query);
    if terms.is_empty() || limit == 0 {
        return Ok(SearchResults::default());
    }

    // Spelling: a word nothing in the index starts with is searched as the
    // closest indexed words.
    let mut corrections = Vec::new();
    let mut exprs = Vec::with_capacity(terms.len());
    for term in &terms {
        let mut alternatives = Vec::new();
        if !term.phrase
            && corrections.len() < MAX_CORRECTED_TERMS
            && !in_vocabulary(conn, term.word())?
        {
            alternatives = corrections_for(conn, term.word())?;
            if let Some(first) = alternatives.first() {
                corrections.push((term.word().to_string(), first.clone()));
            }
        }
        exprs.push(term_expr(term, &alternatives));
    }

    // Which meetings have each term: every term, or else the most.
    let mut coverage: HashMap<MeetingId, usize> = HashMap::new();
    for expr in &exprs {
        for id in meetings_matching(conn, expr)? {
            *coverage.entry(id).or_default() += 1;
        }
    }
    let Some(&most) = coverage.values().max() else {
        return Ok(SearchResults {
            corrections,
            ..Default::default()
        });
    };
    let partial = most < terms.len();
    coverage.retain(|_, n| *n == most);

    // Relevance: every match of any term, by field.
    let any = exprs.join(" OR ");
    let mut scored: HashMap<MeetingId, Scored> = HashMap::new();
    {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT rowid, bm25(meetings_fts, 8.0, 3.0),
                highlight(meetings_fts, 0, '{MARK_OPEN}', '{MARK_CLOSE}'),
                snippet(meetings_fts, 1, '{MARK_OPEN}', '{MARK_CLOSE}', '…', 24)
             FROM meetings_fts WHERE meetings_fts MATCH ?1"
        ))?;
        let mut rows = stmt.query([&any])?;
        while let Some(r) = rows.next()? {
            let id: MeetingId = r.get(0)?;
            if !coverage.contains_key(&id) {
                continue;
            }
            let s = scored.entry(id).or_default();
            s.meeting = -r.get::<_, f64>(1)?;
            s.title = snippet_of(MatchField::Title, &r.get::<_, String>(2)?);
            s.description = r
                .get::<_, Option<String>>(3)?
                .and_then(|d| snippet_of(MatchField::Description, &d));
        }
    }
    {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT rowid, bm25(summaries_fts, 3.0, 2.0, 1.5, 1.5),
                snippet(summaries_fts, -1, '{MARK_OPEN}', '{MARK_CLOSE}', '…', 20)
             FROM summaries_fts WHERE summaries_fts MATCH ?1"
        ))?;
        let mut rows = stmt.query([&any])?;
        while let Some(r) = rows.next()? {
            let id: MeetingId = r.get(0)?;
            if !coverage.contains_key(&id) {
                continue;
            }
            let s = scored.entry(id).or_default();
            s.summary = -r.get::<_, f64>(1)?;
            s.summary_snippet = snippet_of(MatchField::Summary, &r.get::<_, String>(2)?);
        }
    }
    {
        let mut stmt = conn.prepare_cached(
            "SELECT f.rowid, g.meeting_id, g.t0_ms, bm25(segments_fts)
             FROM segments_fts f JOIN meeting_segments g ON g.id = f.rowid
             WHERE segments_fts MATCH ?1",
        )?;
        let mut rows = stmt.query([&any])?;
        while let Some(r) = rows.next()? {
            let id: MeetingId = r.get(1)?;
            if !coverage.contains_key(&id) {
                continue;
            }
            let line = (-r.get::<_, f64>(3)?, r.get(0)?, r.get(2)?);
            scored.entry(id).or_default().lines.push(line);
        }
    }

    // A small nudge for recent meetings: a tie goes to the newer one.
    let now = crate::meetings::store::now_ms();
    let mut started: HashMap<MeetingId, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare_cached("SELECT id, started_at FROM meetings")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        for row in rows {
            let (id, at) = row?;
            started.insert(id, at);
        }
    }
    let mut ranked: Vec<(MeetingId, f64, Scored)> = scored
        .into_iter()
        .map(|(id, s)| {
            let age_days =
                (now - started.get(&id).copied().unwrap_or(now)).max(0) as f64 / 86_400_000.0;
            let score = s.score() * (1.0 + 0.1 * (-age_days / 90.0).exp());
            (id, score, s)
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.1.total_cmp(&a.1).then_with(|| {
            let at = |id| started.get(id).copied().unwrap_or(0);
            at(&b.0).cmp(&at(&a.0))
        })
    });
    ranked.truncate(limit);

    let mut line_stmt = conn.prepare_cached(&format!(
        "SELECT snippet(segments_fts, 0, '{MARK_OPEN}', '{MARK_CLOSE}', '…', 18)
         FROM segments_fts WHERE segments_fts MATCH ?1 AND rowid = ?2"
    ))?;
    let mut hits = Vec::with_capacity(ranked.len());
    for (meeting_id, score, mut s) in ranked {
        s.lines.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut snippets: Vec<Snippet> = Vec::new();
        snippets.extend(s.description.take());
        // The best transcript lines, then the summary: a quote of what was
        // said is what a search for it is usually after.
        for &(_, seg_id, t0) in s.lines.iter().take(MAX_SNIPPETS) {
            let marked: String = line_stmt.query_row(params![any, seg_id], |r| r.get(0))?;
            if let Some(mut sn) = snippet_of(MatchField::Transcript, &marked) {
                sn.segment_id = Some(seg_id);
                sn.t_ms = Some(t0);
                snippets.push(sn);
            }
        }
        if let Some(sum) = s.summary_snippet.take() {
            // The summary goes before a second transcript line.
            let at = snippets.len().min(
                1 + usize::from(
                    snippets
                        .first()
                        .is_some_and(|f| f.field == MatchField::Description),
                ),
            );
            snippets.insert(at, sum);
        }
        snippets.truncate(MAX_SNIPPETS);
        hits.push(SearchHit {
            meeting_id,
            score,
            title: s.title,
            snippets,
            transcript_hits: s.lines.len(),
        });
    }
    Ok(SearchResults {
        hits,
        corrections,
        partial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::store::MeetingsStore;
    use crate::meetings::types::*;

    fn word(t: &str) -> QueryTerm {
        QueryTerm {
            tokens: vec![t.into()],
            phrase: false,
        }
    }

    #[test]
    fn queries_split_like_the_index() {
        assert_eq!(
            parse_query("  SLA, Délai-review  sla "),
            vec![word("sla"), word("délai"), word("review")]
        );
        assert_eq!(
            parse_query("ticket \"cluster office\" x"),
            vec![
                word("ticket"),
                QueryTerm {
                    tokens: vec!["cluster".into(), "office".into()],
                    phrase: true
                }
            ]
        );
        // A lone letter is still a query; an unclosed quote runs to the end.
        assert_eq!(parse_query("a"), vec![word("a")]);
        assert_eq!(parse_query("\"one"), vec![word("one")]);
        assert!(parse_query(" ,.! ").is_empty());
    }

    #[test]
    fn term_expressions_prefer_the_exact_word() {
        assert_eq!(term_expr(&word("sla"), &[]), "(\"sla\" OR \"sla\"*)");
        assert_eq!(
            term_expr(&word("shipmnt"), &["shipment".into()]),
            "(\"shipmnt\" OR \"shipmnt\"* OR \"shipment\")"
        );
        let phrase = QueryTerm {
            tokens: vec!["a".into(), "b".into()],
            phrase: true,
        };
        assert_eq!(term_expr(&phrase, &[]), "\"a b\"");
    }

    #[test]
    fn marked_text_becomes_ranges() {
        let (text, r) = parse_marked("the \u{2}SLA\u{3} is \u{2}85\u{3}%");
        assert_eq!(text, "the SLA is 85%");
        assert_eq!(r, vec![4..7, 11..13]);
        assert_eq!(&text[r[1].clone()], "85");
        let s = snippet_of(
            MatchField::Summary,
            "[\"Ship to \u{2}offices\u{3}\",\"Call\"]",
        )
        .unwrap();
        assert_eq!(s.text, "Ship to offices · Call");
        assert_eq!(&s.text[s.highlights[0].clone()], "offices");
        assert!(snippet_of(MatchField::Title, "no match").is_none());
    }

    #[test]
    fn edit_distance_is_bounded() {
        assert_eq!(edit_distance("shipment", "shipment", 2), Some(0));
        assert_eq!(edit_distance("shipmnet", "shipment", 2), Some(1));
        assert_eq!(edit_distance("shpment", "shipment", 1), Some(1));
        assert_eq!(edit_distance("kitten", "sitting", 2), None);
        assert_eq!(edit_distance("kitten", "sitting", 3), Some(3));
        assert_eq!(edit_distance("a", "abcd", 2), None);
    }

    fn meeting(store: &MeetingsStore, title: &str, started_at: i64) -> MeetingId {
        store
            .insert_meeting(&Meeting {
                id: 0,
                title: title.into(),
                calendar_event_id: None,
                calendar_event: None,
                started_at,
                ended_at: Some(started_at + 60_000),
                mic_audio_path: None,
                system_audio_path: None,
                status: MeetingStatus::Processed,
                error: None,
                processing_stage: None,
                description: None,
                title_source: TitleSource::Default,
                has_summary: false,
            })
            .unwrap()
    }

    fn transcript(store: &MeetingsStore, id: MeetingId, lines: &[&str]) {
        let segments: Vec<Segment> = lines
            .iter()
            .enumerate()
            .map(|(i, text)| Segment {
                id: 0,
                meeting_id: id,
                track: Track::System,
                t0_ms: i as i64 * 10_000,
                t1_ms: i as i64 * 10_000 + 5_000,
                text: text.to_string(),
                words: Vec::new(),
                speaker_id: None,
            })
            .collect();
        store.replace_transcript(id, &[], &segments).unwrap();
    }

    fn summary(store: &MeetingsStore, id: MeetingId, short: &str, topics: &[&str]) {
        store
            .put_summary(&StoredSummary {
                meeting_id: id,
                title: String::new(),
                short_summary: short.into(),
                decisions: Vec::new(),
                topics: topics.iter().map(|t| t.to_string()).collect(),
                generated_by: "test/model".into(),
                generated_at: 1,
                usage: None,

                notes: vec![],
                open_questions: vec![],
            })
            .unwrap();
    }

    fn ids(r: &SearchResults) -> Vec<MeetingId> {
        r.hits.iter().map(|h| h.meeting_id).collect()
    }

    fn fixture() -> (MeetingsStore, MeetingId, MeetingId, MeetingId) {
        let s = MeetingsStore::open_in_memory().unwrap();
        let standup = meeting(&s, "TCPL daily stand-up", 3_000);
        transcript(
            &s,
            standup,
            &[
                "We got ten new tickets today.",
                "The SLA is at eighty five percent.",
                "Call DTDC and push for delivery tomorrow.",
            ],
        );
        summary(
            &s,
            standup,
            "The team reviewed ticket backlog and SLA performance.",
            &["Device shipments and DTDC delays"],
        );
        let weekly = meeting(&s, "NeoCore weekly catch-up", 2_000);
        transcript(&s, weekly, &["Shipment of the new devices slipped a week."]);
        let other = meeting(&s, "Meeting Oct 21, 11:10", 1_000);
        transcript(&s, other, &["Let's talk about the hiring plan."]);
        (s, standup, weekly, other)
    }

    #[test]
    fn finds_meetings_by_title_summary_and_transcript() {
        let (s, standup, weekly, other) = fixture();
        assert_eq!(ids(&s.search("neocore", 10).unwrap()), vec![weekly]);
        assert_eq!(ids(&s.search("hiring", 10).unwrap()), vec![other]);
        // In the summary only.
        assert_eq!(ids(&s.search("backlog", 10).unwrap()), vec![standup]);
        // As you type: a word's start.
        assert_eq!(ids(&s.search("tick", 10).unwrap()), vec![standup]);
        // Accents and case are folded like the index folds them.
        let r = s.search("DÉLAYS", 10).unwrap();
        assert!(r.corrections.is_empty());
        assert_eq!(ids(&r), vec![standup]);
        // Nothing.
        assert!(s.search("zebra", 10).unwrap().hits.is_empty());
        assert!(s.search("   ", 10).unwrap().hits.is_empty());
    }

    #[test]
    fn every_term_must_match_somewhere_in_the_meeting() {
        let (s, standup, _, _) = fixture();
        // "tcpl" is only in the title, "delivery" only in the transcript.
        let r = s.search("tcpl delivery", 10).unwrap();
        assert_eq!(ids(&r), vec![standup]);
        assert!(!r.partial);
        // No meeting has both: the ones with most terms, marked partial.
        let r = s.search("hiring neocore", 10).unwrap();
        assert!(r.partial);
        assert_eq!(r.hits.len(), 2);
    }

    #[test]
    fn typos_are_corrected_from_the_index() {
        let (s, standup, weekly, _) = fixture();
        let r = s.search("shipmnet", 10).unwrap();
        assert_eq!(r.corrections, vec![("shipmnet".into(), "shipment".into())]);
        // "shipment" first; "shipments" (the stand-up's summary) is close too.
        assert_eq!(ids(&r), vec![weekly, standup]);
        let r = s.search("delivry", 10).unwrap();
        assert_eq!(ids(&r), vec![standup]);
        // Short words aren't guessed at.
        assert!(s.search("slx", 10).unwrap().corrections.is_empty());
    }

    #[test]
    fn hits_carry_highlighted_snippets() {
        let (s, standup, _, _) = fixture();
        let r = s.search("dtdc", 10).unwrap();
        let hit = &r.hits[0];
        assert_eq!(hit.meeting_id, standup);
        assert_eq!(hit.transcript_hits, 1);
        assert!(hit.title.is_none());
        let line = hit
            .snippets
            .iter()
            .find(|s| s.field == MatchField::Transcript)
            .unwrap();
        assert_eq!(line.t_ms, Some(20_000));
        assert_eq!(&line.text[line.highlights[0].clone()], "DTDC");
        let sum = hit
            .snippets
            .iter()
            .find(|s| s.field == MatchField::Summary)
            .unwrap();
        assert!(sum.text.contains("DTDC delays"), "{}", sum.text);
        let r = s.search("stand", 10).unwrap();
        let title = r.hits[0].title.as_ref().unwrap();
        assert_eq!(&title.text[title.highlights[0].clone()], "stand");
    }

    #[test]
    fn a_title_match_outranks_a_passing_mention() {
        let s = MeetingsStore::open_in_memory().unwrap();
        let mention = meeting(&s, "Weekly sync", 2_000);
        transcript(&s, mention, &["Someone said budget once.", "Other things."]);
        let titled = meeting(&s, "Budget review", 1_000);
        transcript(&s, titled, &["Numbers."]);
        assert_eq!(ids(&s.search("budget", 10).unwrap()), vec![titled, mention]);
    }

    #[test]
    fn the_index_follows_edits_and_deletes() {
        let (s, standup, weekly, _) = fixture();
        s.set_meeting_title(weekly, "Vendor review", TitleSource::User)
            .unwrap();
        assert!(s.search("neocore", 10).unwrap().hits.is_empty());
        assert_eq!(ids(&s.search("vendor", 10).unwrap()), vec![weekly]);
        s.set_generated_title(
            weekly,
            None,
            ("Vendor review", TitleSource::User),
            "Courier escalations",
            || true,
        )
        .unwrap();
        let r = s.search("courier", 10).unwrap();
        assert_eq!(ids(&r), vec![weekly]);
        assert_eq!(r.hits[0].snippets[0].field, MatchField::Description);
        // A redone transcript replaces the old lines.
        transcript(&s, standup, &["Completely different words."]);
        assert!(s.search("delivery", 10).unwrap().hits.is_empty());
        summary(&s, standup, "Rewritten.", &[]);
        assert!(s.search("backlog", 10).unwrap().hits.is_empty());
        // Deleting the meeting takes its lines and summary along.
        s.delete_meeting(standup).unwrap();
        assert!(s.search("different", 10).unwrap().hits.is_empty());
        assert!(s.search("rewritten", 10).unwrap().hits.is_empty());
    }
}
