//! [`CloudTranscriber`]: meeting transcription through a cloud speech-to-text
//! API, the OpenAI-compatible `POST /audio/transcriptions` that Groq and
//! OpenAI serve ("transcribe again in the cloud").
//!
//! The pipeline hands it consecutive segments of one track at a time (see
//! [`SegmentTranscriber::batch_limit_ms`]). They go up as one WAV with a
//! second of silence between them, so an hour-long meeting is a handful of
//! requests instead of hundreds (free tiers allow ~20 a minute). The reply's
//! words are mapped back to the segment each falls in ([`distribute`]).
//!
//! Neither the transcript nor the API key is ever logged.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Deserialize;

use super::pipeline::SegmentTranscriber;
use super::{Cancelled, SAMPLE_RATE};
use crate::meetings::settings::MeetingSettings;
use crate::meetings::types::{CloudTranscriptionOption, MeetingId, Word};
use crate::settings::{AppSettings, PostProcessProvider};

/// Providers (`post_process_providers` ids) with a speech-to-text API, in
/// the order automatic selection prefers them.
pub const CLOUD_TRANSCRIPTION_PROVIDERS: [&str; 2] = ["groq", "openai"];

/// Silence between the segments of one request.
const GAP_SAMPLES: usize = SAMPLE_RATE;
/// Speech the pipeline batches into one call.
const BATCH_LIMIT_MS: i64 = 10 * 60_000;
/// Audio (gaps included) in one request: 12 minutes of 16-bit 16 kHz WAV is
/// 23 MB, under the APIs' 25 MB upload limit.
const MAX_REQUEST_SAMPLES: usize = 12 * 60 * SAMPLE_RATE;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Tries per request when rate-limited or the server fails.
const MAX_ATTEMPTS: u32 = 3;
/// The longest wait between tries, whatever `Retry-After` asks for.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);

/// A provider's default speech-to-text model.
pub fn default_model(provider_id: &str) -> &'static str {
    match provider_id {
        "groq" => "whisper-large-v3-turbo",
        "openai" => "whisper-1",
        _ => "",
    }
}

/// The model configured for `provider_id`, or its default.
pub fn configured_model(settings: &MeetingSettings, provider_id: &str) -> String {
    settings
        .cloud_transcription_models
        .get(provider_id)
        .map(|m| m.trim())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| default_model(provider_id))
        .to_string()
}

/// The provider to use: the configured one, else (automatic) the first of
/// [`CLOUD_TRANSCRIPTION_PROVIDERS`] with a key, else the first.
pub fn resolve_provider_id(settings: &MeetingSettings, has_key: impl Fn(&str) -> bool) -> String {
    let configured = settings.cloud_transcription_provider_id.trim();
    if CLOUD_TRANSCRIPTION_PROVIDERS.contains(&configured) {
        return configured.to_string();
    }
    CLOUD_TRANSCRIPTION_PROVIDERS
        .into_iter()
        .find(|p| has_key(p))
        .unwrap_or(CLOUD_TRANSCRIPTION_PROVIDERS[0])
        .to_string()
}

/// Every provider a meeting can be transcribed with, the chosen one
/// ([`resolve_provider_id`]) first. `has_key` is asked once per provider.
pub fn transcription_options(
    settings: &AppSettings,
    has_key: impl Fn(&str) -> bool,
) -> Vec<CloudTranscriptionOption> {
    let keys: Vec<(&str, bool)> = CLOUD_TRANSCRIPTION_PROVIDERS
        .into_iter()
        .map(|p| (p, has_key(p)))
        .collect();
    let key_of = |p: &str| keys.iter().any(|(id, k)| *id == p && *k);
    let chosen = resolve_provider_id(&settings.meetings, key_of);
    let mut options: Vec<CloudTranscriptionOption> = CLOUD_TRANSCRIPTION_PROVIDERS
        .into_iter()
        .map(|p| CloudTranscriptionOption {
            provider_id: p.to_string(),
            label: settings
                .post_process_provider(p)
                .map_or_else(|| p.to_string(), |pp| pp.label.clone()),
            model: configured_model(&settings.meetings, p),
            has_key: key_of(p),
        })
        .collect();
    options.sort_by_key(|o| o.provider_id != chosen);
    options
}

/// `provider_id`'s settings entry, when it offers speech-to-text.
pub fn transcription_provider(
    settings: &AppSettings,
    provider_id: &str,
) -> Result<PostProcessProvider> {
    CLOUD_TRANSCRIPTION_PROVIDERS
        .contains(&provider_id)
        .then(|| settings.post_process_provider(provider_id).cloned())
        .flatten()
        .ok_or_else(|| anyhow!("{provider_id} does not offer cloud transcription"))
}

/// No API key is saved for the provider a cloud transcription needs.
/// Typed so the UI can point at the provider's key setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudKeyMissing {
    pub provider_id: String,
    pub label: String,
}

impl std::fmt::Display for CloudKeyMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} has no API key; add one to transcribe with it",
            self.label
        )
    }
}

impl std::error::Error for CloudKeyMissing {}

/// The API answered with an HTTP error status (after any retries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudApiError {
    pub status: u16,
    /// The provider's own message (`error.message`), else the body.
    pub message: String,
    label: String,
}

impl std::fmt::Display for CloudApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} (HTTP {})", self.label, self.message, self.status)
    }
}

impl std::error::Error for CloudApiError {}

/// The `error.message` of an OpenAI-style error body, else the body itself
/// (shortened).
pub(crate) fn api_message(body: &str) -> String {
    let typed = serde_json::from_str::<serde_json::Value>(body).ok();
    let message = typed.as_ref().and_then(|v| {
        let e = v.get("error")?;
        e.get("message")
            .and_then(serde_json::Value::as_str)
            .or_else(|| e.as_str())
            .map(str::to_string)
    });
    let message = message.unwrap_or_else(|| body.trim().chars().take(300).collect());
    if message.trim().is_empty() {
        "no details".to_string()
    } else {
        message
    }
}

/// `verbose_json` with word and segment timestamps (seconds). Words carry
/// no punctuation; segments do.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct VerboseTranscription {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub words: Vec<ApiWord>,
    #[serde(default)]
    pub segments: Vec<ApiSegment>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ApiWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ApiSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// Where one segment sits in a request's audio (ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub offset_ms: i64,
    pub len_ms: i64,
}

fn samples_to_ms(n: usize) -> i64 {
    (n * 1000 / SAMPLE_RATE) as i64
}

/// The segments back to back with [`GAP_SAMPLES`] of silence between them,
/// and where each one landed.
pub fn join_with_gaps(pcms: &[Vec<f32>]) -> (Vec<f32>, Vec<Placement>) {
    let total =
        pcms.iter().map(Vec::len).sum::<usize>() + GAP_SAMPLES * pcms.len().saturating_sub(1);
    let mut audio = Vec::with_capacity(total);
    let mut placements = Vec::with_capacity(pcms.len());
    for (k, pcm) in pcms.iter().enumerate() {
        if k > 0 {
            audio.resize(audio.len() + GAP_SAMPLES, 0.0);
        }
        placements.push(Placement {
            offset_ms: samples_to_ms(audio.len()),
            len_ms: samples_to_ms(pcm.len()),
        });
        audio.extend_from_slice(pcm);
    }
    (audio, placements)
}

/// How many consecutive segments (of the given sample counts) go in each
/// request, so that one request stays under [`MAX_REQUEST_SAMPLES`] with
/// its gaps. A longer segment goes alone.
fn request_sizes(lens: &[usize]) -> Vec<usize> {
    let mut sizes = Vec::new();
    let (mut count, mut samples) = (0usize, 0usize);
    for &len in lens {
        if count > 0 && samples + GAP_SAMPLES + len > MAX_REQUEST_SAMPLES {
            sizes.push(count);
            (count, samples) = (0, 0);
        }
        samples += if count == 0 { len } else { GAP_SAMPLES + len };
        count += 1;
    }
    if count > 0 {
        sizes.push(count);
    }
    sizes
}

/// 16-bit 16 kHz mono WAV.
pub fn encode_wav(pcm: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = Cursor::new(Vec::with_capacity(44 + pcm.len() * 2));
    let mut writer = hound::WavWriter::new(&mut out, spec)?;
    for &s in pcm {
        writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)?;
    }
    writer.finalize()?;
    Ok(out.into_inner())
}

/// The segment a time (ms, request audio) belongs to: the one containing
/// it, else the nearest.
pub(crate) fn placement_at(placements: &[Placement], ms: i64) -> usize {
    let distance = |p: &Placement| {
        if ms < p.offset_ms {
            p.offset_ms - ms
        } else {
            (ms - (p.offset_ms + p.len_ms)).max(0)
        }
    };
    (0..placements.len())
        .min_by_key(|&k| distance(&placements[k]))
        .unwrap_or(0)
}

fn seconds_to_ms(s: f64) -> i64 {
    (s * 1000.0).round() as i64
}

/// Split one request's reply into the segments it was made of: each word
/// goes to the segment its midpoint falls in (the nearest one when it is in
/// a gap), with times relative to that segment's start. Punctuation comes
/// from the segment texts, whose tokens pair with the words by index when
/// the counts match; otherwise the bare words are used. A segment's text is
/// its words joined with spaces. Without words, the reply's segments are
/// split the same way (text only).
pub fn distribute(
    placements: &[Placement],
    reply: &VerboseTranscription,
) -> Vec<(String, Vec<Word>)> {
    if placements.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<(Vec<String>, Vec<Word>)> = vec![Default::default(); placements.len()];
    let relative =
        |k: usize, ms: i64| (ms - placements[k].offset_ms).clamp(0, placements[k].len_ms);
    if !reply.words.is_empty() {
        let joined = reply
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let tokens: Vec<&str> = joined.split_whitespace().collect();
        let punctuated = tokens.len() == reply.words.len();
        for (i, w) in reply.words.iter().enumerate() {
            let text = if punctuated { tokens[i] } else { w.word.trim() };
            if text.is_empty() {
                continue;
            }
            let (t0, t1) = (seconds_to_ms(w.start), seconds_to_ms(w.end.max(w.start)));
            let k = placement_at(placements, (t0 + t1) / 2);
            out[k].0.push(text.to_string());
            out[k].1.push(Word {
                t0_ms: relative(k, t0),
                t1_ms: relative(k, t1),
                text: text.to_string(),
            });
        }
    } else if !reply.segments.is_empty() {
        for s in &reply.segments {
            let mid = (seconds_to_ms(s.start) + seconds_to_ms(s.end)) / 2;
            out[placement_at(placements, mid)]
                .0
                .extend(s.text.split_whitespace().map(str::to_string));
        }
    } else if placements.len() == 1 {
        out[0]
            .0
            .extend(reply.text.split_whitespace().map(str::to_string));
    }
    out.into_iter()
        .map(|(tokens, words)| (tokens.join(" "), words))
        .collect()
}

/// A `multipart/form-data` transcription request: its content type and
/// body.
fn multipart_body(model: &str, language: Option<&str>, wav: &[u8]) -> (String, Vec<u8>) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let boundary = format!("guidy-{nanos:032x}");
    let mut body = Vec::with_capacity(wav.len() + 1024);
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    };
    field("model", model);
    field("response_format", "verbose_json");
    field("timestamp_granularities[]", "word");
    field("timestamp_granularities[]", "segment");
    if let Some(language) = language {
        field("language", language);
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// `Retry-After` in seconds (the date form is ignored).
pub(crate) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let secs: f64 = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

/// Transcribes meeting segments with a cloud provider's speech-to-text API.
pub struct CloudTranscriber {
    provider_id: String,
    label: String,
    base_url: String,
    api_key: String,
    model: String,
    /// ISO-639-1; `None` autodetects.
    language: Option<String>,
    meeting_id: Option<MeetingId>,
    /// The run's cancel flag, checked while a request is in flight.
    cancel: Option<Arc<AtomicBool>>,
    /// First wait before a retry without `Retry-After` (doubles each time).
    retry_base: Duration,
    /// Record each request in the usage ledger (off in tests).
    record_usage: bool,
}

impl CloudTranscriber {
    /// Fails with [`CloudKeyMissing`] when `api_key` is empty. `language` is
    /// the app's language setting (`"auto"` or a code such as `zh-Hans`).
    pub fn new(
        provider: &PostProcessProvider,
        api_key: String,
        model: String,
        language: Option<&str>,
        meeting_id: Option<MeetingId>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Self> {
        if api_key.trim().is_empty() {
            return Err(CloudKeyMissing {
                provider_id: provider.id.clone(),
                label: provider.label.clone(),
            }
            .into());
        }
        let model = Some(model.trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| default_model(&provider.id).to_string());
        let language = language
            .filter(|l| !l.is_empty() && *l != "auto")
            .and_then(|l| l.split(['-', '_']).next())
            .map(str::to_ascii_lowercase);
        Ok(Self {
            provider_id: provider.id.clone(),
            label: provider.label.clone(),
            base_url: provider.base_url.trim_end_matches('/').to_string(),
            api_key: api_key.trim().to_string(),
            model,
            language,
            meeting_id,
            cancel,
            retry_base: Duration::from_secs(2),
            record_usage: true,
        })
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
    }

    /// Send one WAV, stopping (as [`Cancelled`]) when the run is cancelled.
    fn send(&self, wav: Vec<u8>) -> Result<VerboseTranscription> {
        crate::llm::block_on(async move {
            let request = self.request(wav);
            tokio::pin!(request);
            loop {
                if self.cancelled() {
                    return Err(Cancelled.into());
                }
                if let Ok(r) = tokio::time::timeout(Duration::from_millis(200), &mut request).await
                {
                    return r;
                }
            }
        })
    }

    /// The request, retried on 429, 5xx and connection failures.
    async fn request(&self, wav: Vec<u8>) -> Result<VerboseTranscription> {
        let label = &self.label;
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| anyhow!("{label}: could not set up the request: {e}"))?;
        let url = format!("{}/audio/transcriptions", self.base_url);
        let (content_type, body) = multipart_body(&self.model, self.language.as_deref(), &wav);
        drop(wav);
        let mut attempt = 1;
        loop {
            let sent = client
                .post(&url)
                .bearer_auth(&self.api_key)
                .header(reqwest::header::CONTENT_TYPE, &content_type)
                .body(body.clone())
                .send()
                .await;
            let wait = match sent {
                Ok(r) if r.status().is_success() => {
                    let text = r
                        .text()
                        .await
                        .map_err(|e| anyhow!("{label}: could not read the reply: {e}"))?;
                    return serde_json::from_str(&text)
                        .map_err(|e| anyhow!("{label}: unexpected reply: {e}"));
                }
                Ok(r) => {
                    let status = r.status().as_u16();
                    let wait = retry_after(r.headers());
                    let body = r.text().await.unwrap_or_default();
                    let error = CloudApiError {
                        status,
                        message: api_message(&body),
                        label: label.clone(),
                    };
                    if !(status == 429 || status >= 500) || attempt >= MAX_ATTEMPTS {
                        return Err(error.into());
                    }
                    log::warn!(
                        "{label} transcription: HTTP {status}, retrying ({attempt}/{MAX_ATTEMPTS})"
                    );
                    wait.unwrap_or(self.retry_base * 2u32.pow(attempt - 1))
                }
                Err(e) if e.is_connect() && attempt < MAX_ATTEMPTS => {
                    log::warn!("{label} transcription: {e}, retrying ({attempt}/{MAX_ATTEMPTS})");
                    self.retry_base * 2u32.pow(attempt - 1)
                }
                Err(e) => return Err(anyhow!("{label}: the request failed: {e}")),
            };
            tokio::time::sleep(wait.min(MAX_RETRY_WAIT)).await;
            attempt += 1;
        }
    }

    /// One request for `pcms`, split back into one result per segment.
    fn transcribe_joined(&mut self, pcms: &[Vec<f32>]) -> Result<Vec<(String, Vec<Word>)>> {
        let (audio, placements) = join_with_gaps(pcms);
        let audio_ms = samples_to_ms(audio.len()) as u64;
        let wav = encode_wav(&audio)?;
        drop(audio);
        log::info!(
            "meeting {:?}: sending {} segment(s), {} s of audio, to {} ({})",
            self.meeting_id,
            pcms.len(),
            audio_ms / 1000,
            self.label,
            self.model
        );
        let reply = self.send(wav)?;
        if self.record_usage {
            crate::llm::usage::record_transcription(
                &self.provider_id,
                &self.model,
                audio_ms,
                self.meeting_id,
            );
        }
        Ok(distribute(&placements, &reply))
    }
}

impl SegmentTranscriber for CloudTranscriber {
    fn transcribe(&mut self, pcm: Vec<f32>) -> Result<(String, Vec<Word>)> {
        Ok(self.transcribe_batch(vec![pcm])?.pop().unwrap_or_default())
    }

    fn batch_limit_ms(&self) -> Option<i64> {
        Some(BATCH_LIMIT_MS)
    }

    fn transcribe_batch(&mut self, pcms: Vec<Vec<f32>>) -> Result<Vec<(String, Vec<Word>)>> {
        let lens: Vec<usize> = pcms.iter().map(Vec::len).collect();
        let mut pcms = pcms.into_iter();
        let mut out = Vec::with_capacity(lens.len());
        for size in request_sizes(&lens) {
            let part: Vec<Vec<f32>> = pcms.by_ref().take(size).collect();
            out.extend(self.transcribe_joined(&part)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::PostProcessApiFormat;
    use parking_lot::Mutex;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn provider(base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: "groq".into(),
            label: "Groq".into(),
            base_url: base_url.into(),
            allow_base_url_edit: false,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: PostProcessApiFormat::OpenAi,
        }
    }

    fn transcriber(base_url: &str) -> CloudTranscriber {
        let mut t = CloudTranscriber::new(
            &provider(base_url),
            "k".into(),
            String::new(),
            Some("en-US"),
            Some(1),
            None,
        )
        .unwrap();
        t.retry_base = Duration::from_millis(10);
        t.record_usage = false;
        t
    }

    fn ms(n: i64) -> Vec<f32> {
        vec![0.1; n as usize * SAMPLE_RATE / 1000]
    }

    fn word(w: &str, start: f64, end: f64) -> ApiWord {
        ApiWord {
            word: w.into(),
            start,
            end,
        }
    }

    fn segment(text: &str, start: f64, end: f64) -> ApiSegment {
        ApiSegment {
            text: text.into(),
            start,
            end,
        }
    }

    #[test]
    fn wav_round_trips_every_sample() {
        let pcm: Vec<f32> = (0..SAMPLE_RATE + 7)
            .map(|i| (i as f32 / 100.0).sin())
            .collect();
        let wav = encode_wav(&pcm).unwrap();
        assert_eq!(wav.len(), 44 + pcm.len() * 2);
        let reader = hound::WavReader::new(Cursor::new(wav)).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, 16_000, 16)
        );
        assert_eq!(reader.len() as usize, pcm.len());
    }

    #[test]
    fn segments_are_joined_with_a_second_of_silence() {
        let (audio, placements) = join_with_gaps(&[ms(500), ms(300), ms(2000)]);
        assert_eq!(
            audio.len(),
            (500 + 1000 + 300 + 1000 + 2000) * SAMPLE_RATE / 1000
        );
        assert_eq!(
            placements,
            vec![
                Placement {
                    offset_ms: 0,
                    len_ms: 500
                },
                Placement {
                    offset_ms: 1500,
                    len_ms: 300
                },
                Placement {
                    offset_ms: 2800,
                    len_ms: 2000
                },
            ]
        );
        assert_eq!(audio[600 * SAMPLE_RATE / 1000], 0.0);
        assert!(join_with_gaps(&[]).0.is_empty());
    }

    #[test]
    fn requests_stay_under_the_upload_limit() {
        let minute = 60 * SAMPLE_RATE;
        assert_eq!(request_sizes(&[minute; 10]), vec![10]);
        // 11 + 1 minutes of speech plus gaps is over 12 minutes.
        assert_eq!(request_sizes(&[minute; 12]), vec![11, 1]);
        assert_eq!(request_sizes(&[20 * minute, minute]), vec![1, 1]);
        assert!(request_sizes(&[]).is_empty());
    }

    #[test]
    fn words_go_to_the_segment_they_fall_in_with_punctuation() {
        let placements = [
            Placement {
                offset_ms: 0,
                len_ms: 2000,
            },
            Placement {
                offset_ms: 3000,
                len_ms: 2000,
            },
        ];
        let reply = VerboseTranscription {
            text: "Hello there. How are you?".into(),
            words: vec![
                word("Hello", 0.1, 0.5),
                word("there", 0.6, 1.0),
                // In the gap, nearer the second segment.
                word("How", 2.6, 2.9),
                word("are", 3.2, 3.5),
                word("you", 3.6, 4.0),
            ],
            segments: vec![
                segment(" Hello there.", 0.0, 1.0),
                segment(" How are you?", 2.6, 4.0),
            ],
        };
        let out = distribute(&placements, &reply);
        assert_eq!(out[0].0, "Hello there.");
        assert_eq!(out[1].0, "How are you?");
        let first: Vec<(i64, i64, &str)> = out[0]
            .1
            .iter()
            .map(|w| (w.t0_ms, w.t1_ms, w.text.as_str()))
            .collect();
        assert_eq!(first, vec![(100, 500, "Hello"), (600, 1000, "there.")]);
        // Relative to the second segment, clamped to its start.
        let second: Vec<(i64, i64)> = out[1].1.iter().map(|w| (w.t0_ms, w.t1_ms)).collect();
        assert_eq!(second, vec![(0, 0), (200, 500), (600, 1000)]);
    }

    #[test]
    fn mismatched_tokens_fall_back_to_the_bare_words() {
        let placements = [Placement {
            offset_ms: 0,
            len_ms: 3000,
        }];
        let reply = VerboseTranscription {
            text: String::new(),
            words: vec![word("it's", 0.0, 0.3), word("fine", 0.4, 0.8)],
            // "it's" split differently: 3 tokens for 2 words.
            segments: vec![segment("it 's fine.", 0.0, 0.8)],
        };
        let out = distribute(&placements, &reply);
        assert_eq!(out[0].0, "it's fine");
        assert_eq!(out[0].1[1].text, "fine");

        // No words at all: the segments' text, by midpoint.
        let two = [
            Placement {
                offset_ms: 0,
                len_ms: 1000,
            },
            Placement {
                offset_ms: 2000,
                len_ms: 1000,
            },
        ];
        let reply = VerboseTranscription {
            segments: vec![segment(" Yes.", 0.0, 0.9), segment(" No.", 2.1, 2.8)],
            ..Default::default()
        };
        let out = distribute(&two, &reply);
        assert_eq!((out[0].0.as_str(), out[1].0.as_str()), ("Yes.", "No."));
        assert!(out[0].1.is_empty());
        // Silence: nothing anywhere.
        let out = distribute(&two, &VerboseTranscription::default());
        assert_eq!(out, vec![(String::new(), vec![]), (String::new(), vec![])]);
    }

    #[test]
    fn the_provider_is_configured_or_the_first_with_a_key() {
        let mut s = MeetingSettings::default();
        assert_eq!(resolve_provider_id(&s, |_| false), "groq");
        assert_eq!(resolve_provider_id(&s, |p| p == "openai"), "openai");
        assert_eq!(resolve_provider_id(&s, |_| true), "groq");
        s.cloud_transcription_provider_id = "openai".into();
        assert_eq!(resolve_provider_id(&s, |_| false), "openai");
        s.cloud_transcription_provider_id = "anthropic".into();
        assert_eq!(resolve_provider_id(&s, |_| false), "groq");

        let mut app = crate::settings::get_default_settings();
        app.meetings
            .cloud_transcription_models
            .insert("openai".into(), " ".into());
        let options = transcription_options(&app, |p| p == "openai");
        let ids: Vec<(&str, &str, &str, bool)> = options
            .iter()
            .map(|o| {
                (
                    o.provider_id.as_str(),
                    o.label.as_str(),
                    o.model.as_str(),
                    o.has_key,
                )
            })
            .collect();
        assert_eq!(
            ids,
            vec![
                ("openai", "OpenAI", "whisper-1", true),
                ("groq", "Groq", "whisper-large-v3-turbo", false),
            ]
        );
        assert!(transcription_provider(&app, "groq").is_ok());
        assert!(transcription_provider(&app, "anthropic").is_err());
    }

    /// One served reply: status line, extra header lines, body.
    type Reply = (&'static str, &'static str, String);

    /// Serve `replies` in order, one per connection, reading each whole
    /// request (headers and body), which is kept for the test to inspect.
    async fn serve(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            for (status, headers, body) in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buf = vec![0u8; 65536];
                loop {
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                log.lock()
                    .push(String::from_utf8_lossy(&request).into_owned());
                let reply = format!(
                    "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        });
        (format!("http://{address}"), seen)
    }

    fn reply_body() -> String {
        json!({
            "text": "Hi. Bye.",
            "words": [
                {"word": "Hi", "start": 0.1, "end": 0.4},
                {"word": "Bye", "start": 1.6, "end": 1.7},
            ],
            "segments": [
                {"text": " Hi.", "start": 0.0, "end": 0.4},
                {"text": " Bye.", "start": 1.5, "end": 1.8},
            ],
        })
        .to_string()
    }

    #[test]
    fn a_batch_is_one_request_split_back_into_segments() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (base, seen) = rt.block_on(serve(vec![("200 OK", "", reply_body())]));
        let mut t = transcriber(&base);
        assert_eq!(t.batch_limit_ms(), Some(BATCH_LIMIT_MS));
        let out = t.transcribe_batch(vec![ms(500), ms(400)]).unwrap();
        assert_eq!(out[0].0, "Hi.");
        assert_eq!(out[1].0, "Bye.");
        // 1.6 s is 100 ms into the second segment (it starts at 1.5 s).
        assert_eq!((out[1].1[0].t0_ms, out[1].1[0].t1_ms), (100, 200));

        let seen = seen.lock();
        assert_eq!(seen.len(), 1);
        let request = &seen[0];
        assert!(
            request.starts_with("POST /audio/transcriptions "),
            "{request}"
        );
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer k\r\n"));
        for field in [
            "name=\"model\"\r\n\r\nwhisper-large-v3-turbo\r\n",
            "name=\"response_format\"\r\n\r\nverbose_json\r\n",
            "name=\"timestamp_granularities[]\"\r\n\r\nword\r\n",
            "name=\"timestamp_granularities[]\"\r\n\r\nsegment\r\n",
            "name=\"language\"\r\n\r\nen\r\n",
            "filename=\"audio.wav\"",
        ] {
            assert!(request.contains(field), "missing {field}");
        }
    }

    #[test]
    fn rate_limits_are_retried() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let limited = json!({"error": {"message": "Rate limit reached"}}).to_string();
        let (base, seen) = rt.block_on(serve(vec![
            ("429 Too Many Requests", "Retry-After: 0\r\n", limited),
            ("503 Service Unavailable", "", String::new()),
            ("200 OK", "", reply_body()),
        ]));
        let out = transcriber(&base).transcribe(ms(500)).unwrap();
        assert_eq!(out.0, "Hi. Bye.");
        assert_eq!(seen.lock().len(), 3);
    }

    #[test]
    fn api_errors_carry_the_providers_message() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bad_key =
            json!({"error": {"message": "Invalid API Key", "type": "invalid_request_error"}});
        let (base, seen) = rt.block_on(serve(vec![("401 Unauthorized", "", bad_key.to_string())]));
        let e = transcriber(&base).transcribe(ms(500)).unwrap_err();
        let api = e.downcast_ref::<CloudApiError>().unwrap();
        assert_eq!(api.status, 401);
        assert_eq!(e.to_string(), "Groq: Invalid API Key (HTTP 401)");
        // Not retried.
        assert_eq!(seen.lock().len(), 1);

        // Still failing after the last try.
        let (base, seen) = rt.block_on(serve(vec![
            ("500 Internal Server Error", "", "oops".into()),
            ("500 Internal Server Error", "", "oops".into()),
            ("500 Internal Server Error", "", "oops".into()),
        ]));
        let e = transcriber(&base).transcribe(ms(500)).unwrap_err();
        assert_eq!(e.to_string(), "Groq: oops (HTTP 500)");
        assert_eq!(seen.lock().len(), 3);
    }

    #[test]
    fn a_missing_key_is_a_typed_error() {
        let e = CloudTranscriber::new(
            &provider("http://x"),
            " ".into(),
            String::new(),
            None,
            None,
            None,
        )
        .err()
        .unwrap();
        assert_eq!(
            e.downcast_ref::<CloudKeyMissing>(),
            Some(&CloudKeyMissing {
                provider_id: "groq".into(),
                label: "Groq".into()
            })
        );
    }

    #[test]
    fn a_cancelled_run_stops_waiting_for_the_reply() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Accepts, never answers.
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let mut t = transcriber(&base);
        let flag = Arc::new(AtomicBool::new(false));
        t.cancel = Some(flag.clone());
        let raise = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            raise.store(true, Ordering::SeqCst);
        });
        let e = t.transcribe(ms(200)).unwrap_err();
        assert!(super::super::is_cancelled(&e));
        drop(listener);
    }
}
