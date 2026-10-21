//! Wire protocol between Guidy (`guidy_core::llm::builtin`) and this worker.
//!
//! Framing: one JSON object per line (newline-delimited JSON) on stdin
//! (requests) and stdout (responses). JSON string escaping keeps message
//! content on a single line. Blank lines are ignored. Logs go to stderr only,
//! never stdout.
//!
//! The mirror of these types lives in `guidy-core/src/llm/builtin.rs`; keep
//! the two in sync (both sides have round-trip tests on the same JSON).

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Message {
    /// "system", "user" or "assistant".
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Load a GGUF model (replaces any loaded one).
    Load {
        id: u64,
        path: String,
        n_ctx: u32,
        /// Layers to offload to the GPU. `None` = all.
        #[serde(default)]
        n_gpu_layers: Option<u32>,
    },
    /// Generate a reply with the model's chat template.
    Chat {
        id: u64,
        messages: Vec<Message>,
        max_tokens: u32,
        temperature: f32,
        /// Constrain the output to this JSON schema (GBNF grammar).
        #[serde(default)]
        json_schema: Option<Value>,
        /// Send `Token` responses as text is generated.
        #[serde(default)]
        stream: bool,
    },
    /// Stop the running chat (`id: None` = whatever is running). The chat
    /// ends with `Done { finish_reason: Cancelled }`.
    Cancel {
        #[serde(default)]
        id: Option<u64>,
    },
    /// Free the model.
    Unload {
        id: u64,
    },
    Ping {
        id: u64,
    },
    /// Exit the process.
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// End-of-generation token.
    Stop,
    /// `max_tokens` (or the context) ran out.
    Length,
    Cancelled,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Sent once at startup.
    Ready {
        version: String,
    },
    Loaded {
        id: u64,
        n_ctx: u32,
        n_ctx_train: u32,
    },
    Token {
        id: u64,
        text: String,
    },
    Done {
        id: u64,
        text: String,
        finish_reason: FinishReason,
        prompt_tokens: u32,
        completion_tokens: u32,
        /// Time spent on the prompt (prefill), ms. Absent from older helpers.
        #[serde(default)]
        prompt_ms: u64,
        /// Time spent generating, ms.
        #[serde(default)]
        generation_ms: u64,
    },
    Unloaded {
        id: u64,
    },
    Pong {
        id: u64,
    },
    Error {
        id: Option<u64>,
        message: String,
    },
}

/// Read the next frame. `Ok(None)` at end of input; blank lines are skipped.
pub fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut impl BufRead,
) -> std::io::Result<Option<Result<T, serde_json::Error>>> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return Ok(Some(serde_json::from_str(trimmed)));
    }
}

/// Write one frame (a single line) and flush.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_round_trip_one_per_line() {
        let requests = vec![
            Request::Load {
                id: 1,
                path: "/m/model.gguf".into(),
                n_ctx: 8192,
                n_gpu_layers: None,
            },
            Request::Chat {
                id: 2,
                messages: vec![Message {
                    role: "user".into(),
                    content: "line one\nline two \"quoted\"".into(),
                }],
                max_tokens: 64,
                temperature: 0.2,
                json_schema: Some(serde_json::json!({"type": "object"})),
                stream: true,
            },
            Request::Cancel { id: Some(2) },
            Request::Shutdown,
        ];
        let mut buf = Vec::new();
        for r in &requests {
            write_frame(&mut buf, r).unwrap();
        }
        // Multi-line content stays inside one frame.
        assert_eq!(buf.iter().filter(|&&b| b == b'\n').count(), requests.len());

        let mut cursor = Cursor::new(buf);
        let mut back = Vec::new();
        while let Some(frame) = read_frame::<Request>(&mut cursor).unwrap() {
            back.push(frame.unwrap());
        }
        assert_eq!(back, requests);
    }

    #[test]
    fn blank_lines_are_skipped_and_bad_json_is_reported() {
        let mut cursor = Cursor::new(b"\n  \n{\"op\":\"ping\",\"id\":7}\nnot json\n".to_vec());
        let first = read_frame::<Request>(&mut cursor)
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(first, Request::Ping { id: 7 });
        assert!(read_frame::<Request>(&mut cursor)
            .unwrap()
            .unwrap()
            .is_err());
        assert!(read_frame::<Request>(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn wire_format_is_stable() {
        let json = serde_json::to_value(Request::Cancel { id: None }).unwrap();
        assert_eq!(json, serde_json::json!({"op": "cancel", "id": null}));
        // Optional fields may be omitted by the sender.
        let chat: Request = serde_json::from_str(
            r#"{"op":"chat","id":3,"messages":[],"max_tokens":5,"temperature":0}"#,
        )
        .unwrap();
        assert!(matches!(
            chat,
            Request::Chat {
                json_schema: None,
                stream: false,
                ..
            }
        ));

        let done = Response::Done {
            id: 3,
            text: "hi".into(),
            finish_reason: FinishReason::Stop,
            prompt_tokens: 4,
            completion_tokens: 1,
            prompt_ms: 12,
            generation_ms: 30,
        };
        assert_eq!(
            serde_json::to_value(&done).unwrap(),
            serde_json::json!({"type":"done","id":3,"text":"hi","finish_reason":"stop",
                "prompt_tokens":4,"completion_tokens":1,"prompt_ms":12,"generation_ms":30})
        );
    }
}
