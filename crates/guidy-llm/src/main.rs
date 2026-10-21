//! Guidy's built-in llama.cpp worker (see docs/MEETING_NOTES_ARCH.md).
//!
//! A separate process because two static copies of ggml (this one and
//! transcribe-cpp's) can't link into one binary. The app spawns it lazily from
//! `guidy_core::llm::builtin`, talks newline-delimited JSON over stdin/stdout
//! ([`protocol`]), and kills it after an idle period or on cancel.
//!
//! One request runs at a time on the main thread. A reader thread parses
//! stdin so a `Cancel` can interrupt a running `Chat` between tokens.

mod protocol;

use std::io::{BufReader, Write};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use protocol::{read_frame, write_frame, FinishReason, Message, Request, Response};

/// `cancel_id` value meaning "cancel whatever is running".
const CANCEL_ALL: u64 = u64::MAX;
/// Prompt tokens decoded per batch.
const N_BATCH: u32 = 512;

type Out = Arc<Mutex<std::io::Stdout>>;

fn send(out: &Out, response: &Response) {
    let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = write_frame(&mut *out, response) {
        // stdout is gone: the parent died. Nothing left to do.
        eprintln!("guidy-llm: failed to write response: {e}");
        std::process::exit(0);
    }
}

fn main() {
    let out: Out = Arc::new(Mutex::new(std::io::stdout()));
    let mut backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            send(
                &out,
                &Response::Error {
                    id: None,
                    message: format!("llama.cpp backend init failed: {e}"),
                },
            );
            std::process::exit(1);
        }
    };
    if std::env::var_os("GUIDY_LLM_VERBOSE").is_none() {
        backend.void_logs();
    }

    // Id of the chat to cancel (0 = none, CANCEL_ALL = any).
    let cancel_id = Arc::new(AtomicU64::new(0));
    let (tx, rx) = mpsc::channel::<Request>();
    {
        let cancel_id = cancel_id.clone();
        let out = out.clone();
        std::thread::spawn(move || {
            let mut stdin = BufReader::new(std::io::stdin());
            loop {
                match read_frame::<Request>(&mut stdin) {
                    Ok(Some(Ok(Request::Cancel { id }))) => {
                        cancel_id.store(id.unwrap_or(CANCEL_ALL), Ordering::SeqCst);
                    }
                    Ok(Some(Ok(request))) => {
                        if tx.send(request).is_err() {
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => send(
                        &out,
                        &Response::Error {
                            id: None,
                            message: format!("malformed request: {e}"),
                        },
                    ),
                    // EOF or a broken pipe: the parent is gone.
                    Ok(None) | Err(_) => {
                        cancel_id.store(CANCEL_ALL, Ordering::SeqCst);
                        let _ = tx.send(Request::Shutdown);
                        break;
                    }
                }
            }
        });
    }

    send(
        &out,
        &Response::Ready {
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    );

    let mut worker = Worker {
        backend,
        model: None,
        n_ctx: 0,
    };
    while let Ok(request) = rx.recv() {
        match request {
            Request::Load {
                id,
                path,
                n_ctx,
                n_gpu_layers,
            } => match worker.load(&path, n_ctx, n_gpu_layers) {
                Ok(n_ctx_train) => send(
                    &out,
                    &Response::Loaded {
                        id,
                        n_ctx: worker.n_ctx,
                        n_ctx_train,
                    },
                ),
                Err(e) => send(
                    &out,
                    &Response::Error {
                        id: Some(id),
                        message: format!("{e:#}"),
                    },
                ),
            },
            Request::Chat {
                id,
                messages,
                max_tokens,
                temperature,
                json_schema,
                stream,
            } => {
                let is_cancelled = || {
                    let c = cancel_id.load(Ordering::SeqCst);
                    c == id || c == CANCEL_ALL
                };
                let on_token = |text: &str| {
                    if stream {
                        send(
                            &out,
                            &Response::Token {
                                id,
                                text: text.to_string(),
                            },
                        );
                    }
                };
                let result = worker.chat(
                    &messages,
                    max_tokens,
                    temperature,
                    json_schema.as_ref(),
                    &is_cancelled,
                    &on_token,
                );
                // A cancel aimed at this chat is consumed now.
                let _ = cancel_id.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
                let _ =
                    cancel_id.compare_exchange(CANCEL_ALL, 0, Ordering::SeqCst, Ordering::SeqCst);
                match result {
                    Ok(done) => send(
                        &out,
                        &Response::Done {
                            id,
                            text: done.text,
                            finish_reason: done.finish_reason,
                            prompt_tokens: done.prompt_tokens,
                            completion_tokens: done.completion_tokens,
                            prompt_ms: done.prompt_ms,
                            generation_ms: done.generation_ms,
                        },
                    ),
                    Err(e) => send(
                        &out,
                        &Response::Error {
                            id: Some(id),
                            message: format!("{e:#}"),
                        },
                    ),
                }
            }
            Request::Unload { id } => {
                worker.model = None;
                send(&out, &Response::Unloaded { id });
            }
            Request::Ping { id } => send(&out, &Response::Pong { id }),
            Request::Cancel { .. } => {}
            Request::Shutdown => break,
        }
    }
    drop(worker);
    let _ = out.lock().map(|mut o| o.flush());
}

struct Worker {
    backend: LlamaBackend,
    model: Option<LlamaModel>,
    n_ctx: u32,
}

struct ChatDone {
    text: String,
    finish_reason: FinishReason,
    prompt_tokens: u32,
    completion_tokens: u32,
    prompt_ms: u64,
    generation_ms: u64,
}

impl Worker {
    fn load(&mut self, path: &str, n_ctx: u32, n_gpu_layers: Option<u32>) -> Result<u32> {
        // Free the old model first so two never sit in memory at once.
        self.model = None;
        if !std::path::Path::new(path).is_file() {
            bail!("model file not found: {path}");
        }
        let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers.unwrap_or(999));
        let model = LlamaModel::load_from_file(&self.backend, path, &params)
            .with_context(|| format!("failed to load model {path}"))?;
        let n_ctx_train = model.n_ctx_train();
        self.n_ctx = if n_ctx_train > 0 {
            n_ctx.clamp(512, n_ctx_train)
        } else {
            n_ctx.max(512)
        };
        self.model = Some(model);
        Ok(n_ctx_train)
    }

    fn chat(
        &mut self,
        messages: &[Message],
        max_tokens: u32,
        temperature: f32,
        json_schema: Option<&serde_json::Value>,
        is_cancelled: &dyn Fn() -> bool,
        on_token: &dyn Fn(&str),
    ) -> Result<ChatDone> {
        let model = self
            .model
            .as_ref()
            .ok_or_else(|| anyhow!("no model loaded"))?;
        let prompt = render_prompt(model, messages)?;

        let vocab = model.vocab();
        let mut tokens = vocab.tokenize(prompt.as_bytes(), false, true);
        if vocab.should_add_bos() && tokens.first() != Some(&vocab.bos()) {
            tokens.insert(0, vocab.bos());
        }
        let n_prompt = tokens.len() as u32;
        if n_prompt + 16 > self.n_ctx {
            bail!(
                "prompt too long: {n_prompt} tokens for a {} token context",
                self.n_ctx
            );
        }
        let max_tokens = max_tokens.min(self.n_ctx - n_prompt).max(1);

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.n_ctx))
            .with_n_batch(N_BATCH)
            .with_n_threads(threads())
            .with_n_threads_batch(threads());
        let mut ctx = model
            .new_context(&self.backend, ctx_params)
            .context("failed to create a llama.cpp context")?;

        let mut sampler = build_sampler(model, temperature, json_schema)?;

        // Prompt, in batches; logits only for the very last token.
        let started = Instant::now();
        let mut batch = LlamaBatch::new(N_BATCH as usize, 1);
        let last = tokens.len() - 1;
        for (chunk_index, chunk) in tokens.chunks(N_BATCH as usize).enumerate() {
            if is_cancelled() {
                return Ok(cancelled(String::new(), n_prompt, 0));
            }
            batch.clear();
            for (j, &token) in chunk.iter().enumerate() {
                let pos = chunk_index * N_BATCH as usize + j;
                batch.add(token, pos as i32, &[0], pos == last)?;
            }
            ctx.decode(&mut batch).context("prompt decode failed")?;
        }
        let prompt_ms = started.elapsed().as_millis() as u64;
        let generating = Instant::now();

        let mut bytes: Vec<u8> = Vec::new();
        let mut streamed = 0usize;
        let mut n_cur = tokens.len() as i32;
        let mut generated = 0u32;
        let finish_reason = loop {
            if is_cancelled() {
                break FinishReason::Cancelled;
            }
            if generated >= max_tokens {
                break FinishReason::Length;
            }
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if vocab.is_eog(token) {
                break FinishReason::Stop;
            }
            bytes.extend(vocab.token_to_piece(token, false, None));
            generated += 1;

            // Stream whatever is valid UTF-8 so far (a token can end mid-character).
            let valid = match std::str::from_utf8(&bytes[streamed..]) {
                Ok(s) => s.len(),
                Err(e) => e.valid_up_to(),
            };
            if valid > 0 {
                on_token(std::str::from_utf8(&bytes[streamed..streamed + valid]).unwrap_or(""));
                streamed += valid;
            }

            batch.clear();
            batch.add(token, n_cur, &[0], true)?;
            n_cur += 1;
            ctx.decode(&mut batch).context("decode failed")?;
        };

        let text = String::from_utf8_lossy(&bytes).into_owned();
        if finish_reason == FinishReason::Cancelled {
            return Ok(cancelled(text, n_prompt, generated));
        }
        Ok(ChatDone {
            text,
            finish_reason,
            prompt_tokens: n_prompt,
            completion_tokens: generated,
            prompt_ms,
            generation_ms: generating.elapsed().as_millis() as u64,
        })
    }
}

fn cancelled(text: String, prompt_tokens: u32, completion_tokens: u32) -> ChatDone {
    ChatDone {
        text,
        finish_reason: FinishReason::Cancelled,
        prompt_tokens,
        completion_tokens,
        prompt_ms: 0,
        generation_ms: 0,
    }
}

fn threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 8) as i32)
        .unwrap_or(4)
}

/// Apply the GGUF's own chat template (ChatML if it has none).
fn render_prompt(model: &LlamaModel, messages: &[Message]) -> Result<String> {
    let template = match model.chat_template(None) {
        Ok(t) => t,
        Err(_) => LlamaChatTemplate::new("chatml")?,
    };
    let chat = messages
        .iter()
        .map(|m| LlamaChatMessage::new(m.role.clone(), m.content.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut prompt = model
        .apply_chat_template(&template, &chat, true)
        .context("failed to apply the model's chat template")?;
    // Hybrid thinking models (Qwen3): an empty think block is how their own
    // template switches thinking off (`enable_thinking=false`).
    if template
        .to_str()
        .map(|t| t.contains("enable_thinking"))
        .unwrap_or(false)
    {
        prompt.push_str("<think>\n\n</think>\n\n");
    }
    Ok(prompt)
}

fn build_sampler(
    model: &LlamaModel,
    temperature: f32,
    json_schema: Option<&serde_json::Value>,
) -> Result<LlamaSampler> {
    let mut chain = Vec::new();
    if let Some(schema) = json_schema {
        let grammar = llama_cpp_2::json_schema_to_grammar(&schema.to_string())
            .map_err(|e| anyhow!("unsupported JSON schema: {e}"))?;
        chain.push(
            LlamaSampler::grammar(model, &grammar, "root")
                .map_err(|e| anyhow!("invalid grammar: {e}"))?,
        );
    }
    if temperature <= 0.0 {
        chain.push(LlamaSampler::greedy());
    } else {
        chain.push(LlamaSampler::top_k(40));
        chain.push(LlamaSampler::top_p(0.95, 1));
        chain.push(LlamaSampler::min_p(0.05, 1));
        chain.push(LlamaSampler::temp(temperature));
        chain.push(LlamaSampler::dist(0x6775_6964));
    }
    Ok(LlamaSampler::chain_simple(chain))
}
