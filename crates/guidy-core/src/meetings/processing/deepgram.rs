//! [`Deepgram`]: who spoke when, from Deepgram's pre-recorded API with
//! `diarize=true`, for "redo speakers in the cloud".
//!
//! The system track's speech segments go up as one WAV (a second of silence
//! between them, as [`super::cloud`] does), so the speaker numbers are
//! consistent across the whole meeting: one request, however long. Each
//! segment gets the speaker who said most of its words, or none when no one
//! clearly did (two voices in one segment): an unknown speaker for the user
//! to name rather than a guess. The transcript itself is kept as it is.
//!
//! Neither the audio's words nor the API key are logged.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Deserialize;

use super::cloud::{api_message, encode_wav, join_with_gaps, placement_at, retry_after, Placement};
use super::pipeline::SpeakerTurns;
use crate::meetings::types::MeetingId;

/// The key name in [`crate::secrets`] (`provider_api_key("deepgram")`).
pub const DEEPGRAM_PROVIDER_ID: &str = "deepgram";
pub const DEEPGRAM_LABEL: &str = "Deepgram";
pub const DEEPGRAM_MODEL: &str = "nova-3";
const DEEPGRAM_URL: &str = "https://api.deepgram.com";

/// A segment gets a speaker only when they said at least this share of it.
const MIN_SHARE: f64 = 0.7;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_ATTEMPTS: u32 = 3;

#[derive(Deserialize, Debug, Default)]
struct Response {
    results: Results,
}

#[derive(Deserialize, Debug, Default)]
struct Results {
    #[serde(default)]
    channels: Vec<Channel>,
}

#[derive(Deserialize, Debug, Default)]
struct Channel {
    #[serde(default)]
    alternatives: Vec<Alternative>,
}

#[derive(Deserialize, Debug, Default)]
struct Alternative {
    #[serde(default)]
    words: Vec<DgWord>,
}

/// One word with its speaker (seconds).
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DgWord {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub speaker: Option<usize>,
}

/// The words of a `/v1/listen` reply.
pub fn reply_words(body: &str) -> Result<Vec<DgWord>> {
    let r: Response =
        serde_json::from_str(body).map_err(|e| anyhow!("Deepgram: unexpected reply: {e}"))?;
    Ok(r.results
        .channels
        .into_iter()
        .next()
        .and_then(|c| c.alternatives.into_iter().next())
        .map(|a| a.words)
        .unwrap_or_default())
}

/// The speaker of each segment (`placements` in the request's audio): the
/// one who said most of its words by time, when they said at least
/// [`MIN_SHARE`] of it; `None` for a segment without words or with no clear
/// speaker.
pub fn segment_speakers(placements: &[Placement], words: &[DgWord]) -> Vec<Option<usize>> {
    let mut talk: Vec<std::collections::HashMap<usize, f64>> =
        vec![Default::default(); placements.len()];
    for w in words {
        let Some(speaker) = w.speaker else { continue };
        let mid = ((w.start + w.end) / 2.0 * 1000.0).round() as i64;
        let k = placement_at(placements, mid);
        *talk[k].entry(speaker).or_default() += (w.end - w.start).max(0.01);
    }
    talk.into_iter()
        .map(|by| {
            let total: f64 = by.values().sum();
            let (speaker, t) = by
                .into_iter()
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))?;
            (t / total >= MIN_SHARE).then_some(speaker)
        })
        .collect()
}

/// Deepgram's `err_msg`, else the OpenAI-style message or the body.
fn error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("err_msg")?.as_str().map(str::to_string))
        .unwrap_or_else(|| api_message(body))
}

/// Deepgram's speaker turns for a meeting's system audio.
pub struct Deepgram {
    api_key: String,
    base_url: String,
    meeting_id: Option<MeetingId>,
    /// Record the request in the usage ledger (off in tests).
    record_usage: bool,
    retry_base: Duration,
}

impl Deepgram {
    /// Fails with [`super::CloudKeyMissing`] without a key.
    pub fn new(api_key: String, meeting_id: Option<MeetingId>) -> Result<Self> {
        if api_key.trim().is_empty() {
            return Err(super::CloudKeyMissing {
                provider_id: DEEPGRAM_PROVIDER_ID.into(),
                label: DEEPGRAM_LABEL.into(),
            }
            .into());
        }
        Ok(Self {
            api_key: api_key.trim().to_string(),
            base_url: DEEPGRAM_URL.into(),
            meeting_id,
            record_usage: true,
            retry_base: Duration::from_secs(2),
        })
    }

    async fn request(&self, wav: Vec<u8>) -> Result<String> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Deepgram: could not set up the request: {e}"))?;
        let url = format!(
            "{}/v1/listen?model={DEEPGRAM_MODEL}&diarize=true&punctuate=true",
            self.base_url.trim_end_matches('/')
        );
        let mut attempt = 1;
        loop {
            let sent = client
                .post(&url)
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Token {}", self.api_key),
                )
                .header(reqwest::header::CONTENT_TYPE, "audio/wav")
                .body(wav.clone())
                .send()
                .await;
            let wait = match sent {
                Ok(r) if r.status().is_success() => {
                    return r
                        .text()
                        .await
                        .map_err(|e| anyhow!("Deepgram: could not read the reply: {e}"));
                }
                Ok(r) => {
                    let status = r.status().as_u16();
                    let wait = retry_after(r.headers());
                    let message = error_message(&r.text().await.unwrap_or_default());
                    if !(status == 429 || status >= 500) || attempt >= MAX_ATTEMPTS {
                        return Err(anyhow!("Deepgram: {message} (HTTP {status})"));
                    }
                    log::warn!("Deepgram: HTTP {status}, retrying ({attempt}/{MAX_ATTEMPTS})");
                    wait.unwrap_or(self.retry_base * 2u32.pow(attempt - 1))
                }
                Err(e) if e.is_connect() && attempt < MAX_ATTEMPTS => {
                    self.retry_base * 2u32.pow(attempt - 1)
                }
                Err(e) => return Err(anyhow!("Deepgram: the request failed: {e}")),
            };
            tokio::time::sleep(wait.min(Duration::from_secs(60))).await;
            attempt += 1;
        }
    }
}

impl SpeakerTurns for Deepgram {
    fn speakers(&mut self, pcms: Vec<Vec<f32>>) -> Result<Vec<Option<usize>>> {
        if pcms.is_empty() {
            return Ok(Vec::new());
        }
        let (audio, placements) = join_with_gaps(&pcms);
        let audio_ms = (audio.len() * 1000 / super::SAMPLE_RATE) as u64;
        let wav = encode_wav(&audio)?;
        drop(audio);
        log::info!(
            "meeting {:?}: sending {} segment(s), {} s of audio, to Deepgram for speakers",
            self.meeting_id,
            pcms.len(),
            audio_ms / 1000
        );
        let body = crate::llm::block_on(self.request(wav))?;
        if self.record_usage {
            crate::llm::usage::record_transcription(
                DEEPGRAM_PROVIDER_ID,
                DEEPGRAM_MODEL,
                audio_ms,
                self.meeting_id,
            );
        }
        Ok(segment_speakers(&placements, &reply_words(&body)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn word(start: f64, end: f64, speaker: usize) -> DgWord {
        DgWord {
            start,
            end,
            speaker: Some(speaker),
        }
    }

    #[test]
    fn each_segment_gets_the_speaker_who_said_most_of_it() {
        // Segments at 0-3 s, 4-6 s, 7-10 s of the request.
        let placements = [
            Placement {
                offset_ms: 0,
                len_ms: 3_000,
            },
            Placement {
                offset_ms: 4_000,
                len_ms: 2_000,
            },
            Placement {
                offset_ms: 7_000,
                len_ms: 3_000,
            },
        ];
        let words = [
            word(0.1, 0.5, 0),
            word(0.6, 2.5, 0),
            word(4.1, 5.0, 1),
            word(5.0, 5.8, 1),
            // Two voices, half each: nobody's.
            word(7.1, 8.4, 0),
            word(8.5, 9.8, 1),
        ];
        assert_eq!(
            segment_speakers(&placements, &words),
            vec![Some(0), Some(1), None]
        );
        // A segment without words has no speaker.
        assert_eq!(segment_speakers(&placements[..1], &[]), vec![None]);
    }

    #[test]
    fn the_reply_words_are_read() {
        let body = r#"{"metadata":{},"results":{"channels":[{"alternatives":[
            {"transcript":"hi there","words":[
              {"word":"hi","start":0.1,"end":0.3,"speaker":0},
              {"word":"there","start":0.4,"end":0.7,"speaker":1}]}]}]}}"#;
        assert_eq!(
            reply_words(body).unwrap(),
            vec![word(0.1, 0.3, 0), word(0.4, 0.7, 1)]
        );
        assert!(reply_words("not json").is_err());
    }

    /// Answers each request with the next of `replies` (status, body) and
    /// returns the base URL and the request heads seen.
    async fn serve(
        replies: Vec<(u16, String)>,
    ) -> (String, std::sync::Arc<parking_lot::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            for (status, body) in replies {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 65536];
                // Read the head, then the body by Content-Length.
                loop {
                    let n = sock.read(&mut chunk).await.unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        let len = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        while buf.len() < end + 4 + len {
                            let n = sock.read(&mut chunk).await.unwrap();
                            buf.extend_from_slice(&chunk[..n]);
                        }
                        log.lock().push(head);
                        break;
                    }
                }
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                sock.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{addr}"), seen)
    }

    fn client(base_url: String) -> Deepgram {
        Deepgram {
            base_url,
            record_usage: false,
            retry_base: Duration::from_millis(1),
            ..Deepgram::new("dg-key".into(), Some(1)).unwrap()
        }
    }

    #[test]
    fn speakers_come_from_one_request_with_retries() {
        let reply = r#"{"results":{"channels":[{"alternatives":[{"words":[
            {"word":"a","start":0.1,"end":0.9,"speaker":0},
            {"word":"b","start":2.2,"end":2.8,"speaker":1}]}]}]}}"#;
        let (url, seen) = crate::host::async_runtime::block_on(serve(vec![
            (429, r#"{"err_msg":"slow down"}"#.into()),
            (200, reply.into()),
        ]));
        let mut dg = client(url);
        // 1 s of speech, then (after the 1 s gap) 1 s more.
        let got = dg
            .speakers(vec![vec![0.1; 16_000], vec![0.1; 16_000]])
            .unwrap();
        assert_eq!(got, vec![Some(0), Some(1)]);
        let heads = seen.lock().clone();
        assert_eq!(heads.len(), 2);
        assert!(heads[1].contains("diarize=true"), "{}", heads[1]);
        assert!(heads[1]
            .to_ascii_lowercase()
            .contains("authorization: token dg-key"));
    }

    #[test]
    fn an_error_is_reported_with_the_providers_message() {
        let (url, _) = crate::host::async_runtime::block_on(serve(vec![(
            401,
            r#"{"err_code":"INVALID_AUTH","err_msg":"Invalid credentials."}"#.into(),
        )]));
        let err = client(url).speakers(vec![vec![0.1; 16_000]]).unwrap_err();
        assert!(
            format!("{err:#}").contains("Invalid credentials"),
            "{err:#}"
        );
        assert!(Deepgram::new(" ".into(), None).is_err());
    }
}
