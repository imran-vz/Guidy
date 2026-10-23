//! What cloud LLM calls cost: token counts from each response, priced with
//! the provider's reported cost (OpenRouter and some gateways return one) or
//! the registry's per-million-token prices, kept in a small local ledger
//! (`llm_usage.db` in the app data dir) for the Usage page.
//!
//! Local models are free, so the same ledger keeps what they did instead
//! ([`LocalRun`]): audio transcribed and how much faster than real time, and
//! the built-in LLM's tokens, generation speed and load time.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Token counts of one or more calls, plus the cost the provider reported
/// for them (when every call reported one).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// USD, as reported by the provider (`usage.cost`).
    pub reported_cost_usd: Option<f64>,
}

impl TokenUsage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0 && self.reported_cost_usd.is_none()
    }

    /// Add another call's usage.
    pub fn add(&mut self, other: &TokenUsage) {
        let first = self.is_empty();
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.reported_cost_usd = match (first, self.reported_cost_usd, other.reported_cost_usd) {
            (true, _, c) => c,
            (false, Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
    }
}

/// Filled by `llm_client` with the usage of the calls it makes.
pub type UsageSlot = Arc<Mutex<TokenUsage>>;

/// The usage object of an OpenAI-style (`prompt_tokens`, `completion_tokens`,
/// `cost`) or Anthropic (`input_tokens` + cache tokens, `output_tokens`)
/// response.
pub fn parse_usage(usage: &serde_json::Value) -> Option<TokenUsage> {
    let n = |k: &str| usage.get(k).and_then(serde_json::Value::as_u64);
    let input = n("prompt_tokens").or_else(|| {
        n("input_tokens").map(|i| {
            i + n("cache_creation_input_tokens").unwrap_or(0)
                + n("cache_read_input_tokens").unwrap_or(0)
        })
    });
    let output = n("completion_tokens").or_else(|| n("output_tokens"));
    let cost = usage
        .get("cost")
        .and_then(serde_json::Value::as_f64)
        .filter(|c| c.is_finite() && *c >= 0.0);
    if input.is_none() && output.is_none() && cost.is_none() {
        return None;
    }
    Some(TokenUsage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        reported_cost_usd: cost,
    })
}

/// A call's cost: `estimated` when priced from the registry rather than
/// reported by the provider; `None` when neither knows the price.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct Cost {
    pub usd: Option<f64>,
    pub estimated: bool,
}

/// Price `usage` for `model` on `provider_id`.
pub fn price(provider_id: &str, model: &str, usage: &TokenUsage) -> Cost {
    if let Some(usd) = usage.reported_cost_usd {
        return Cost {
            usd: Some(usd),
            estimated: false,
        };
    }
    let registry = super::registry::current();
    let rates = registry.find(provider_id, model).and_then(|m| m.cost);
    price_with(rates, usage)
}

fn price_with(rates: Option<super::registry::Cost>, usage: &TokenUsage) -> Cost {
    let usd = rates.and_then(|r| {
        let (input, output) = (r.input?, r.output.unwrap_or(0.0));
        Some(
            (usage.input_tokens as f64 * input + usage.output_tokens as f64 * output) / 1_000_000.0,
        )
    });
    Cost {
        usd,
        estimated: usd.is_some(),
    }
}

/// What a call was for.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UsageFeature {
    /// Dictation (transcription and post-processing).
    Dictation,
    MeetingTranscription,
    MeetingSummary,
    /// Anything else (ticket drafts, connection tests, ...).
    #[default]
    Other,
}

impl UsageFeature {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::MeetingTranscription => "meeting_transcription",
            Self::MeetingSummary => "meeting_summary",
            Self::Other => "other",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "dictation" => Self::Dictation,
            "meeting_transcription" => Self::MeetingTranscription,
            "meeting_summary" => Self::MeetingSummary,
            _ => Self::Other,
        }
    }
}

/// Totals for one job (e.g. a meeting summary's several calls), passed in
/// `ChatOptions::meter`. Every call it sees is also recorded in the ledger.
#[derive(Clone, Debug, Default)]
pub struct UsageMeter {
    pub feature: UsageFeature,
    pub meeting_id: Option<i64>,
    totals: Arc<Mutex<MeterTotals>>,
}

/// What a [`UsageMeter`] saw.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct MeterTotals {
    pub calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Sum of the priced calls; `None` when no call could be priced.
    pub cost_usd: Option<f64>,
    /// Some call was priced from the registry, or couldn't be priced.
    pub estimated: bool,
}

impl UsageMeter {
    pub fn new(feature: UsageFeature, meeting_id: Option<i64>) -> Self {
        Self {
            feature,
            meeting_id,
            totals: Default::default(),
        }
    }

    pub fn totals(&self) -> MeterTotals {
        *self.totals.lock()
    }

    /// A local model's call: tokens, no cost.
    pub fn add_local(&self, input_tokens: u64, output_tokens: u64) {
        let mut t = self.totals.lock();
        t.calls += 1;
        t.input_tokens += input_tokens;
        t.output_tokens += output_tokens;
    }

    fn add(&self, usage: &TokenUsage, cost: Cost) {
        let mut t = self.totals.lock();
        t.calls += 1;
        t.input_tokens += usage.input_tokens;
        t.output_tokens += usage.output_tokens;
        if let Some(usd) = cost.usd {
            t.cost_usd = Some(t.cost_usd.unwrap_or(0.0) + usd);
        }
        t.estimated |= cost.estimated || cost.usd.is_none();
    }
}

/// Price one finished call, add it to `meter` and record it in the ledger.
/// Returns its cost.
pub fn record_call(
    provider_id: &str,
    model: &str,
    usage: &TokenUsage,
    meter: Option<&UsageMeter>,
    feature: UsageFeature,
) -> Cost {
    let cost = price(provider_id, model, usage);
    if let Some(m) = meter {
        m.add(usage, cost);
    }
    let record = UsageRecord {
        at_ms: crate::meetings::store::now_ms(),
        feature: meter.map_or(feature, |m| m.feature),
        provider_id: provider_id.to_string(),
        model: model.to_string(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cost,
        meeting_id: meter.and_then(|m| m.meeting_id),
    };
    if let Err(e) = ledger_record(&record) {
        log::warn!("LLM usage not recorded: {e:#}");
    }
    cost
}

// ----- cloud transcription -----------------------------------------------------

/// Cloud speech-to-text prices, USD per audio minute. Transcription is billed
/// by audio length, not tokens, so these aren't in the models.dev registry.
const TRANSCRIPTION_USD_PER_MIN: &[(&str, &str, f64)] = &[
    ("groq", "whisper-large-v3-turbo", 0.04 / 60.0),
    ("groq", "whisper-large-v3", 0.111 / 60.0),
    ("openai", "whisper-1", CLOUD_TRANSCRIPTION_USD_PER_MIN),
    // Nova-3 pre-recorded ($0.0043) plus the diarization add-on (~$0.002).
    ("deepgram", "nova-3", 0.0063),
];

/// The price of `model` on `provider_id`, USD per audio minute.
pub fn transcription_usd_per_min(provider_id: &str, model: &str) -> Option<f64> {
    TRANSCRIPTION_USD_PER_MIN
        .iter()
        .find(|(p, m, _)| *p == provider_id && *m == model)
        .map(|(_, _, usd)| *usd)
}

/// Price `audio_ms` of audio sent to a cloud speech-to-text model.
pub fn price_transcription(provider_id: &str, model: &str, audio_ms: u64) -> Cost {
    let usd = transcription_usd_per_min(provider_id, model).map(|r| audio_ms as f64 / 60_000.0 * r);
    Cost {
        usd,
        estimated: usd.is_some(),
    }
}

/// Record one cloud transcription request (`audio_ms` uploaded) in the
/// ledger, as a call with no tokens. Returns its cost.
pub fn record_transcription(
    provider_id: &str,
    model: &str,
    audio_ms: u64,
    meeting_id: Option<i64>,
) -> Cost {
    let cost = price_transcription(provider_id, model, audio_ms);
    let record = UsageRecord {
        at_ms: crate::meetings::store::now_ms(),
        feature: UsageFeature::MeetingTranscription,
        provider_id: provider_id.to_string(),
        model: model.to_string(),
        input_tokens: 0,
        output_tokens: 0,
        cost,
        meeting_id,
    };
    if let Err(e) = ledger_record(&record) {
        log::warn!("transcription usage not recorded: {e:#}");
    }
    cost
}

// ----- local models ------------------------------------------------------------

/// What runs a local model.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalKind {
    /// Speech to text (transcribe.cpp).
    #[default]
    Transcription,
    /// The built-in LLM runtime (llama.cpp).
    Llm,
}

impl LocalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transcription => "transcription",
            Self::Llm => "llm",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "llm" => Self::Llm,
            _ => Self::Transcription,
        }
    }
}

/// One job a local model did: a dictation, a meeting's transcription, or
/// one built-in LLM call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocalRun {
    pub kind: LocalKind,
    pub feature: UsageFeature,
    pub model: String,
    /// Speech transcribed.
    pub audio_ms: u64,
    /// Words in the transcript.
    pub words: u64,
    /// LLM prompt tokens (prefill).
    pub input_tokens: u64,
    /// LLM generated tokens.
    pub output_tokens: u64,
    /// Time the model spent working (loading excluded). `None` when not
    /// measurable, e.g. a dictation decoded live while it was recorded.
    pub compute_ms: Option<u64>,
    /// The LLM's share of `compute_ms` spent on the prompt (prefill); the
    /// rest is generation.
    pub prompt_ms: u64,
    /// Time spent loading the model for this job (0 = already loaded).
    pub load_ms: u64,
    pub meeting_id: Option<i64>,
}

/// Record a local model's job in the ledger.
pub fn record_local_run(run: &LocalRun) {
    if let Err(e) = ledger_record_local(crate::meetings::store::now_ms(), run) {
        log::warn!("local model usage not recorded: {e:#}");
    }
}

/// What the Usage page cites as the cloud price of transcription, USD per
/// audio minute (OpenAI `whisper-1` / `gpt-4o-transcribe`).
pub const CLOUD_TRANSCRIPTION_USD_PER_MIN: f64 = 0.006;

/// One local model's work, over every recorded job.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct LocalModelUsage {
    pub kind: LocalKind,
    pub model: String,
    pub runs: u64,
    pub audio_ms: u64,
    pub words: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Sum of the measured compute times.
    pub compute_ms: u64,
    /// The LLM's prompt processing (prefill) share of `compute_ms`.
    pub prompt_ms: u64,
    /// Audio of the jobs with a measured compute time (the RTFx numerator).
    pub timed_audio_ms: u64,
    /// Jobs that loaded the model, and the time that took.
    pub loads: u64,
    pub load_ms: u64,
    pub last_used_ms: i64,
}

impl LocalModelUsage {
    /// How many times faster than real time transcription ran (RTFx: audio
    /// duration / compute time; loading excluded).
    pub fn realtime_factor(&self) -> Option<f64> {
        (self.compute_ms > 0 && self.timed_audio_ms > 0)
            .then(|| self.timed_audio_ms as f64 / self.compute_ms as f64)
    }

    /// LLM generation speed (decode), tokens per second.
    pub fn generation_tokens_per_second(&self) -> Option<f64> {
        let ms = self.compute_ms.saturating_sub(self.prompt_ms);
        (ms > 0 && self.output_tokens > 0).then(|| self.output_tokens as f64 * 1000.0 / ms as f64)
    }

    /// LLM prompt processing speed (prefill), tokens per second.
    pub fn prompt_tokens_per_second(&self) -> Option<f64> {
        (self.prompt_ms > 0 && self.input_tokens > 0)
            .then(|| self.input_tokens as f64 * 1000.0 / self.prompt_ms as f64)
    }

    /// Mean model load time.
    pub fn avg_load_ms(&self) -> Option<u64> {
        (self.loads > 0).then(|| self.load_ms / self.loads)
    }
}

/// Local work per feature.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct LocalFeatureUsage {
    pub feature: UsageFeature,
    pub runs: u64,
    pub audio_ms: u64,
    pub output_tokens: u64,
}

/// The local half of the Usage page.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct LocalUsageReport {
    /// Most used first.
    pub models: Vec<LocalModelUsage>,
    pub features: Vec<LocalFeatureUsage>,
    pub runs: u64,
    pub audio_ms: u64,
    pub words: u64,
    pub tokens: u64,
    /// Transcription and loading time, all models.
    pub compute_ms: u64,
}

impl LocalUsageReport {
    /// What the transcribed audio would have cost at
    /// [`CLOUD_TRANSCRIPTION_USD_PER_MIN`].
    pub fn cloud_transcription_usd(&self) -> f64 {
        self.audio_ms as f64 / 60_000.0 * CLOUD_TRANSCRIPTION_USD_PER_MIN
    }
}

// ----- the ledger ------------------------------------------------------------

/// File name of the ledger inside the app data dir.
pub const USAGE_DB_FILE: &str = "llm_usage.db";

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRecord {
    pub at_ms: i64,
    pub feature: UsageFeature,
    pub provider_id: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: Cost,
    pub meeting_id: Option<i64>,
}

/// Spend on one model (provider + model id), over every recorded call.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ModelSpend {
    pub provider_id: String,
    pub model: String,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Sum over the priced calls.
    pub total_usd: f64,
    /// Mean cost of a priced call.
    pub avg_usd: Option<f64>,
    /// The most expensive call.
    pub max_usd: Option<f64>,
    /// Calls with no known price.
    pub unpriced_calls: u64,
    /// Calls priced from the registry rather than reported by the provider.
    pub estimated_calls: u64,
    pub last_used_ms: i64,
}

/// Spend per feature.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct FeatureSpend {
    pub feature: UsageFeature,
    pub calls: u64,
    pub total_usd: f64,
}

/// Everything the Usage page shows.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct UsageReport {
    /// By total spend, highest first.
    pub models: Vec<ModelSpend>,
    pub features: Vec<FeatureSpend>,
    pub total_usd: f64,
    pub calls: u64,
    /// Spend in the last 30 days.
    pub last_30_days_usd: f64,
    pub local: LocalUsageReport,
}

fn ledger() -> &'static Mutex<Option<Connection>> {
    static L: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(None))
}

fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    create_schema(&conn)?;
    Ok(conn)
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS llm_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at_ms INTEGER NOT NULL,
            feature TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cost_usd REAL,
            cost_estimated INTEGER NOT NULL DEFAULT 0,
            meeting_id INTEGER
        );
        CREATE INDEX IF NOT EXISTS llm_calls_model ON llm_calls(provider_id, model);
        CREATE TABLE IF NOT EXISTS local_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at_ms INTEGER NOT NULL,
            kind TEXT NOT NULL,
            feature TEXT NOT NULL,
            model TEXT NOT NULL,
            audio_ms INTEGER NOT NULL DEFAULT 0,
            words INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            compute_ms INTEGER,
            prompt_ms INTEGER NOT NULL DEFAULT 0,
            load_ms INTEGER NOT NULL DEFAULT 0,
            meeting_id INTEGER
        );
        CREATE INDEX IF NOT EXISTS local_runs_model ON local_runs(kind, model);",
    )?;
    Ok(())
}

/// Open the ledger in `app_data_dir`. Until this runs, calls aren't recorded.
pub fn init(app_data_dir: &Path) -> Result<()> {
    let conn = open(&app_data_dir.join(USAGE_DB_FILE))?;
    *ledger().lock() = Some(conn);
    Ok(())
}

#[cfg(test)]
pub(crate) fn init_in_memory() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn).unwrap();
    *ledger().lock() = Some(conn);
}

fn ledger_record(r: &UsageRecord) -> Result<()> {
    let guard = ledger().lock();
    let Some(conn) = guard.as_ref() else {
        return Ok(());
    };
    conn.execute(
        "INSERT INTO llm_calls (at_ms, feature, provider_id, model, input_tokens,
            output_tokens, cost_usd, cost_estimated, meeting_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            r.at_ms,
            r.feature.as_str(),
            r.provider_id,
            r.model,
            r.input_tokens as i64,
            r.output_tokens as i64,
            r.cost.usd,
            r.cost.estimated,
            r.meeting_id,
        ],
    )?;
    Ok(())
}

fn ledger_record_local(at_ms: i64, r: &LocalRun) -> Result<()> {
    let guard = ledger().lock();
    let Some(conn) = guard.as_ref() else {
        return Ok(());
    };
    conn.execute(
        "INSERT INTO local_runs (at_ms, kind, feature, model, audio_ms, words,
            input_tokens, output_tokens, compute_ms, prompt_ms, load_ms, meeting_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            at_ms,
            r.kind.as_str(),
            r.feature.as_str(),
            r.model,
            r.audio_ms as i64,
            r.words as i64,
            r.input_tokens as i64,
            r.output_tokens as i64,
            r.compute_ms.map(|ms| ms as i64),
            r.prompt_ms as i64,
            r.load_ms as i64,
            r.meeting_id,
        ],
    )?;
    Ok(())
}

fn local_report(conn: &Connection) -> Result<LocalUsageReport> {
    let mut stmt = conn.prepare(
        "SELECT kind, model, COUNT(*), SUM(audio_ms), SUM(words), SUM(input_tokens),
            SUM(output_tokens), COALESCE(SUM(compute_ms), 0),
            COALESCE(SUM(CASE WHEN compute_ms IS NOT NULL THEN audio_ms END), 0),
            SUM(load_ms > 0), SUM(load_ms), MAX(at_ms), SUM(prompt_ms)
         FROM local_runs GROUP BY kind, model
         ORDER BY 3 DESC, 2",
    )?;
    let u = |r: &rusqlite::Row, i| r.get::<_, i64>(i).map(|n| n as u64);
    let models = stmt
        .query_map([], |r| {
            Ok(LocalModelUsage {
                kind: LocalKind::parse(&r.get::<_, String>(0)?),
                model: r.get(1)?,
                runs: u(r, 2)?,
                audio_ms: u(r, 3)?,
                words: u(r, 4)?,
                input_tokens: u(r, 5)?,
                output_tokens: u(r, 6)?,
                compute_ms: u(r, 7)?,
                timed_audio_ms: u(r, 8)?,
                loads: u(r, 9)?,
                load_ms: u(r, 10)?,
                last_used_ms: r.get(11)?,
                prompt_ms: u(r, 12)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare(
        "SELECT feature, COUNT(*), SUM(audio_ms), SUM(output_tokens) FROM local_runs
         GROUP BY feature ORDER BY 2 DESC",
    )?;
    let features = stmt
        .query_map([], |r| {
            Ok(LocalFeatureUsage {
                feature: UsageFeature::parse(&r.get::<_, String>(0)?),
                runs: u(r, 1)?,
                audio_ms: u(r, 2)?,
                output_tokens: u(r, 3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(LocalUsageReport {
        runs: models.iter().map(|m| m.runs).sum(),
        audio_ms: models.iter().map(|m| m.audio_ms).sum(),
        words: models.iter().map(|m| m.words).sum(),
        tokens: models
            .iter()
            .map(|m| m.input_tokens + m.output_tokens)
            .sum(),
        compute_ms: models.iter().map(|m| m.compute_ms + m.load_ms).sum(),
        models,
        features,
    })
}

/// The Usage page's numbers. Empty when the ledger isn't open.
pub fn report() -> Result<UsageReport> {
    let guard = ledger().lock();
    let Some(conn) = guard.as_ref() else {
        return Ok(UsageReport::default());
    };
    let mut stmt = conn.prepare(
        "SELECT provider_id, model, COUNT(*), SUM(input_tokens), SUM(output_tokens),
            COALESCE(SUM(cost_usd), 0), AVG(cost_usd), MAX(cost_usd),
            SUM(cost_usd IS NULL), SUM(cost_usd IS NOT NULL AND cost_estimated), MAX(at_ms)
         FROM llm_calls GROUP BY provider_id, model
         ORDER BY 6 DESC, 3 DESC",
    )?;
    let models = stmt
        .query_map([], |r| {
            Ok(ModelSpend {
                provider_id: r.get(0)?,
                model: r.get(1)?,
                calls: r.get::<_, i64>(2)? as u64,
                input_tokens: r.get::<_, i64>(3)? as u64,
                output_tokens: r.get::<_, i64>(4)? as u64,
                total_usd: r.get(5)?,
                avg_usd: r.get(6)?,
                max_usd: r.get(7)?,
                unpriced_calls: r.get::<_, i64>(8)? as u64,
                estimated_calls: r.get::<_, i64>(9)? as u64,
                last_used_ms: r.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare(
        "SELECT feature, COUNT(*), COALESCE(SUM(cost_usd), 0) FROM llm_calls
         GROUP BY feature ORDER BY 3 DESC",
    )?;
    let features = stmt
        .query_map([], |r| {
            Ok(FeatureSpend {
                feature: UsageFeature::parse(&r.get::<_, String>(0)?),
                calls: r.get::<_, i64>(1)? as u64,
                total_usd: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let since = crate::meetings::store::now_ms() - 30 * 24 * 3600 * 1000;
    let last_30_days_usd: f64 = conn.query_row(
        "SELECT COALESCE(SUM(cost_usd), 0) FROM llm_calls WHERE at_ms >= ?1",
        [since],
        |r| r.get(0),
    )?;
    Ok(UsageReport {
        total_usd: models.iter().map(|m| m.total_usd).sum(),
        calls: models.iter().map(|m| m.calls).sum(),
        models,
        features,
        last_30_days_usd,
        local: local_report(conn)?,
    })
}

/// Forget every recorded call and local run.
pub fn clear() -> Result<()> {
    if let Some(conn) = ledger().lock().as_ref() {
        conn.execute_batch("DELETE FROM llm_calls; DELETE FROM local_runs;")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_openai_anthropic_and_reported_costs() {
        assert_eq!(
            parse_usage(
                &json!({"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120})
            ),
            Some(TokenUsage {
                input_tokens: 100,
                output_tokens: 20,
                reported_cost_usd: None
            })
        );
        assert_eq!(
            parse_usage(&json!({"input_tokens": 10, "cache_read_input_tokens": 90,
                "cache_creation_input_tokens": 5, "output_tokens": 7})),
            Some(TokenUsage {
                input_tokens: 105,
                output_tokens: 7,
                reported_cost_usd: None
            })
        );
        assert_eq!(
            parse_usage(&json!({"prompt_tokens": 1, "completion_tokens": 2, "cost": 0.0042}))
                .unwrap()
                .reported_cost_usd,
            Some(0.0042)
        );
        assert_eq!(parse_usage(&json!({})), None);
    }

    #[test]
    fn reported_costs_add_up_only_when_every_call_has_one() {
        let mut u = TokenUsage::default();
        let call = |c| TokenUsage {
            input_tokens: 1,
            output_tokens: 1,
            reported_cost_usd: c,
        };
        u.add(&call(Some(0.5)));
        u.add(&call(Some(0.25)));
        assert_eq!(u.reported_cost_usd, Some(0.75));
        u.add(&call(None));
        assert_eq!(u.reported_cost_usd, None);
        assert_eq!(u.input_tokens, 3);
    }

    #[test]
    fn prices_from_the_registry_rates_per_million_tokens() {
        let usage = TokenUsage {
            input_tokens: 2_000,
            output_tokens: 500,
            reported_cost_usd: None,
        };
        let rates = super::super::registry::Cost {
            input: Some(2.0),
            output: Some(10.0),
        };
        let c = price_with(Some(rates), &usage);
        assert!((c.usd.unwrap() - 0.009).abs() < 1e-12);
        assert!(c.estimated);
        assert_eq!(price_with(None, &usage), Cost::default());
        let reported = TokenUsage {
            reported_cost_usd: Some(0.01),
            ..usage
        };
        assert_eq!(
            price("unknown", "x", &reported),
            Cost {
                usd: Some(0.01),
                estimated: false
            }
        );
    }

    /// The ledger is global: its tests take turns.
    static LEDGER: Mutex<()> = Mutex::new(());

    #[test]
    fn the_ledger_reports_spend_per_model_and_feature() {
        let _turn = LEDGER.lock();
        init_in_memory();
        clear().unwrap();
        let meter = UsageMeter::new(UsageFeature::MeetingSummary, Some(4));
        let usage = |c| TokenUsage {
            input_tokens: 100,
            output_tokens: 10,
            reported_cost_usd: c,
        };
        record_call(
            "openrouter",
            "a",
            &usage(Some(0.02)),
            Some(&meter),
            UsageFeature::Other,
        );
        record_call(
            "openrouter",
            "a",
            &usage(Some(0.04)),
            Some(&meter),
            UsageFeature::Other,
        );
        record_call("custom", "b", &usage(None), None, UsageFeature::Dictation);
        let t = meter.totals();
        assert_eq!(t.calls, 2);
        assert!((t.cost_usd.unwrap() - 0.06).abs() < 1e-12);
        assert!(!t.estimated);

        let r = report().unwrap();
        assert_eq!(r.calls, 3);
        assert!((r.total_usd - 0.06).abs() < 1e-12);
        let a = &r.models[0];
        assert_eq!((a.model.as_str(), a.calls), ("a", 2));
        assert!((a.avg_usd.unwrap() - 0.03).abs() < 1e-12);
        assert!((a.max_usd.unwrap() - 0.04).abs() < 1e-12);
        let b = &r.models[1];
        assert_eq!((b.unpriced_calls, b.avg_usd), (1, None));
        assert_eq!(r.features[0].feature, UsageFeature::MeetingSummary);
    }

    #[test]
    fn the_ledger_reports_local_work_per_model() {
        let _turn = LEDGER.lock();
        init_in_memory();
        clear().unwrap();
        let asr = |audio_ms, compute_ms, load_ms| LocalRun {
            kind: LocalKind::Transcription,
            feature: UsageFeature::Dictation,
            model: "nemotron".into(),
            audio_ms,
            words: 10,
            compute_ms,
            load_ms,
            ..Default::default()
        };
        // 60 s in 3 s, after a 2 s load; then 30 s decoded live (untimed).
        record_local_run(&asr(60_000, Some(3_000), 2_000));
        record_local_run(&asr(30_000, None, 0));
        record_local_run(&LocalRun {
            kind: LocalKind::Llm,
            feature: UsageFeature::MeetingSummary,
            model: "qwen".into(),
            input_tokens: 900,
            output_tokens: 100,
            compute_ms: Some(4_000),
            prompt_ms: 1_000,
            meeting_id: Some(3),
            ..Default::default()
        });
        let r = report().unwrap().local;
        assert_eq!(
            (r.runs, r.audio_ms, r.words, r.tokens),
            (3, 90_000, 20, 1_000)
        );
        let nemotron = &r.models[0];
        assert_eq!((nemotron.model.as_str(), nemotron.runs), ("nemotron", 2));
        assert_eq!(nemotron.realtime_factor(), Some(20.0));
        assert_eq!(nemotron.avg_load_ms(), Some(2_000));
        assert_eq!(nemotron.generation_tokens_per_second(), None);
        let qwen = &r.models[1];
        assert_eq!(qwen.kind, LocalKind::Llm);
        // 900 prompt tokens in 1 s, 100 generated in 3 s.
        assert_eq!(qwen.prompt_tokens_per_second(), Some(900.0));
        assert!((qwen.generation_tokens_per_second().unwrap() - 33.333).abs() < 1e-3);
        assert_eq!(qwen.realtime_factor(), None);
        assert!((r.cloud_transcription_usd() - 0.009).abs() < 1e-12);
        assert_eq!(r.features[0].feature, UsageFeature::Dictation);
        clear().unwrap();
        assert_eq!(report().unwrap().local.runs, 0);
    }

    #[test]
    fn cloud_transcription_is_priced_per_audio_minute() {
        let _turn = LEDGER.lock();
        init_in_memory();
        clear().unwrap();
        let hour = 3_600_000;
        let turbo = price_transcription("groq", "whisper-large-v3-turbo", hour);
        assert!((turbo.usd.unwrap() - 0.04).abs() < 1e-12);
        assert!(turbo.estimated);
        let large = price_transcription("groq", "whisper-large-v3", hour);
        assert!((large.usd.unwrap() - 0.111).abs() < 1e-12);
        assert_eq!(
            price_transcription("groq", "unknown", hour),
            Cost::default()
        );

        // Ten minutes on OpenAI, in the meeting's transcription spend.
        let cost = record_transcription("openai", "whisper-1", 600_000, Some(9));
        assert!((cost.usd.unwrap() - 0.06).abs() < 1e-12);
        let r = report().unwrap();
        assert_eq!(r.calls, 1);
        assert!((r.total_usd - 0.06).abs() < 1e-12);
        let m = &r.models[0];
        assert_eq!(
            (m.provider_id.as_str(), m.model.as_str()),
            ("openai", "whisper-1")
        );
        assert_eq!((m.input_tokens, m.estimated_calls), (0, 1));
        assert_eq!(r.features[0].feature, UsageFeature::MeetingTranscription);
        clear().unwrap();
    }
}
