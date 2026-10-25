//! [`HttpProvider`]: any `post_process_providers` entry (OpenAI-compatible or
//! Anthropic format) as an [`LlmProvider`], through [`crate::llm_client`].

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde_json::Value;

use super::usage::{self, UsageFeature, UsageSlot};
use super::{
    block_on, extract_json, json_via_prompt, strip_think, validate_json, ChatMessage, ChatOptions,
    LlmProvider, CANCELLED,
};
use crate::llm_client::{self, ChatError, ChatParams, ModelInfo};
use crate::settings::{
    PostProcessApiFormat, PostProcessProvider, CLOUDFLARE_WORKERS_AI_PROVIDER_ID,
};

pub struct HttpProvider {
    provider: PostProcessProvider,
    api_key: String,
    model: String,
}

impl HttpProvider {
    pub fn new(provider: PostProcessProvider, api_key: String, model: String) -> Self {
        Self {
            provider,
            api_key,
            model,
        }
    }

    fn params(&self, messages: &[ChatMessage], opts: &ChatOptions) -> ChatParams {
        ChatParams {
            messages: messages
                .iter()
                .map(|m| (m.role.as_str().to_string(), m.content.clone()))
                .collect(),
            json_schema: None,
            max_tokens: opts.max_tokens,
            temperature: opts.temperature,
            // Same policy as dictation post-processing (actions.rs).
            disable_reasoning: matches!(
                self.provider.id.as_str(),
                "custom" | "openrouter" | "openai" | "zai"
            ),
            usage: Some(UsageSlot::default()),
        }
    }

    fn send(&self, params: ChatParams, opts: &ChatOptions) -> Result<String> {
        let provider = self.provider.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let cancel = opts.cancel.clone();
        let slot = params.usage.clone();
        let reply = block_on(async move {
            let request = llm_client::send_chat(&provider, api_key, &model, params);
            tokio::pin!(request);
            loop {
                if cancel
                    .as_ref()
                    .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
                {
                    return Err(ChatError::Failed(CANCELLED.to_string()));
                }
                if let Ok(r) = tokio::time::timeout(Duration::from_millis(200), &mut request).await
                {
                    return r;
                }
            }
        });
        // Spent tokens count even when the reply is unusable (cut short).
        if let Some(usage) = slot.map(|s| *s.lock()).filter(|u| !u.is_empty()) {
            usage::record_call(
                &self.provider.id,
                &self.model,
                &usage,
                opts.meter.as_ref(),
                UsageFeature::Other,
            );
        }
        let label = &self.provider.label;
        match reply {
            Ok(Some(text)) => Ok(strip_think(&text).to_string()),
            Ok(None) => Err(ReplyCutShort(format!("{label} returned an empty reply")).into()),
            Err(ChatError::Truncated(e)) => Err(ReplyCutShort(format!("{label}: {e}")).into()),
            Err(ChatError::NoChoices) => Err(anyhow!("{label} returned an empty reply")),
            Err(ChatError::Failed(e)) if e == CANCELLED => Err(anyhow!(CANCELLED)),
            Err(ChatError::Status { status, body }) => {
                let message = format!(
                    "{label}: {}",
                    ChatError::Status {
                        status,
                        body: body.clone()
                    }
                );
                Err(ApiStatusError {
                    status,
                    body,
                    message,
                }
                .into())
            }
            Err(e) => Err(anyhow!("{label}: {e}")),
        }
    }
}

/// The endpoint answered with an HTTP error status. Typed so `chat_json` can
/// tell a rejected structured-output request from other failures.
#[derive(Debug)]
pub struct ApiStatusError {
    pub status: u16,
    pub body: String,
    message: String,
}

impl std::fmt::Display for ApiStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiStatusError {}

/// Whether a failed structured-output request was the endpoint rejecting the
/// `response_format` / JSON schema itself (so a prompt-only retry can work),
/// rather than an auth, rate-limit, server, network, truncation or
/// context-overflow failure, which the retry would only repeat.
///
/// OpenAI-style typed error fields decide when present (`error.param` naming
/// `response_format` / `text.format`, or `error.code` `invalid_json_schema`);
/// matching the body text is the last resort, and a bare mention of "schema"
/// (e.g. an invalid tool schema) doesn't count.
fn structured_output_rejected(e: &anyhow::Error) -> bool {
    let Some(rejection) = e.downcast_ref::<ApiStatusError>() else {
        return false;
    };
    if !matches!(rejection.status, 400 | 422) {
        return false;
    }
    let lower = rejection.body.to_ascii_lowercase();
    let overflow = [
        "context length",
        "context_length",
        "context window",
        "too many tokens",
        "maximum context",
        "exceeds the context",
        "prompt too long",
    ];
    if overflow.iter().any(|p| lower.contains(p)) {
        return false;
    }

    let typed = serde_json::from_str::<Value>(&rejection.body).ok();
    let error = typed.as_ref().and_then(|v| v.get("error"));
    let field = |name: &str| {
        error
            .and_then(|e| e.get(name))
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
    };
    if let Some(param) = field("param") {
        // The endpoint named the rejected field: trust it either way.
        return param == "response_format"
            || param.starts_with("response_format.")
            || param == "text.format"
            || param.starts_with("text.format.");
    }
    if field("code").as_deref() == Some("invalid_json_schema") {
        return true;
    }

    [
        "response_format",
        "json_schema",
        "structured output",
        "structured_output",
        "text.format",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

/// Endpoints (see [`llm_client::endpoint_key`]) that rejected structured
/// output, remembered for the lifetime of the process so later requests go
/// straight to prompt-only JSON.
fn structured_output_rejections() -> &'static Mutex<HashSet<String>> {
    static REJECTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REJECTED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A reply that got through but was cut off at the output token limit or
/// came back with an empty message after a normal stop (other finish
/// reasons, like OpenRouter's "error", are failures). A typed error so the connection test can
/// tell such a probe from a failed one (see `super::test_connection`).
#[derive(Debug)]
pub struct ReplyCutShort(String);

impl std::fmt::Display for ReplyCutShort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReplyCutShort {}

/// Context window when nothing is known about a model: small enough for
/// most hosted and local models, so long transcripts are chunked rather than
/// rejected.
pub const DEFAULT_CONTEXT_TOKENS: usize = 16_000;

/// Context windows reported by model lists, per provider (id + base URL) and
/// model. A provider key with an empty map was listed without metadata, so it
/// isn't fetched again. Failed listings aren't cached here (see
/// [`failed_listings`]).
fn context_cache() -> &'static Mutex<HashMap<String, HashMap<String, usize>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, HashMap<String, usize>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// How long after a failed model listing [`HttpProvider::context_tokens`]
/// waits before trying again.
const LISTING_RETRY_AFTER: Duration = Duration::from_secs(5 * 60);

/// When each provider's last model listing for context sizes failed.
fn failed_listings() -> &'static Mutex<HashMap<String, Instant>> {
    static FAILED: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    FAILED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn listing_backed_off(provider: &PostProcessProvider) -> bool {
    failed_listings()
        .lock()
        .get(&cache_key(provider))
        .is_some_and(|at| at.elapsed() < LISTING_RETRY_AFTER)
}

fn cache_key(provider: &PostProcessProvider) -> String {
    format!(
        "{}|{}",
        provider.id,
        provider.base_url.trim().trim_end_matches('/')
    )
}

/// Record the context windows a model list reported.
pub fn remember_model_info(provider: &PostProcessProvider, models: &[ModelInfo]) {
    let mut cache = context_cache().lock();
    let entry = cache.entry(cache_key(provider)).or_default();
    for m in models {
        if let Some(n) = m.context_length {
            entry.insert(m.id.clone(), n);
        }
    }
}

fn cached_context(provider: &PostProcessProvider, model: &str) -> Option<Option<usize>> {
    let cache = context_cache().lock();
    cache
        .get(&cache_key(provider))
        .map(|models| models.get(model).copied())
}

/// Providers whose `/models` list reports context windows: worth one listing
/// the first time a model's context is needed.
fn lists_context_lengths(provider: &PostProcessProvider) -> bool {
    matches!(provider.id.as_str(), "openrouter" | "vercel_ai_gateway")
}

/// Context window for `model` on `provider`: the model list's metadata when
/// it has been seen (see [`remember_model_info`]), else
/// [`context_tokens_by_name`] for a model it knows, else the registry's
/// size.
pub fn context_tokens_for(provider: &PostProcessProvider, model: &str) -> usize {
    cached_context(provider, model)
        .flatten()
        // The registry lists a model's maximum, and hosted variants are
        // often smaller: it only fills in for models the name table doesn't
        // know.
        .or_else(|| known_context_by_name(&provider.id, model))
        .or_else(|| registry_context(provider, model))
        .unwrap_or(DEFAULT_CONTEXT_TOKENS)
}

/// The context window models.dev lists for this provider's model.
fn registry_context(provider: &PostProcessProvider, model: &str) -> Option<usize> {
    let tokens = super::registry::current()
        .find(&provider.id, model)?
        .limit
        .context?;
    usize::try_from(tokens).ok().filter(|t| *t > 0)
}

/// Conservative context windows of well-known models, by name. Hosted
/// variants are often smaller than the model's maximum, so these are the
/// common hosted sizes, not the largest ones.
pub fn context_tokens_by_name(provider_id: &str, model: &str) -> usize {
    known_context_by_name(provider_id, model).unwrap_or(DEFAULT_CONTEXT_TOKENS)
}

/// [`context_tokens_by_name`], `None` for a model the table doesn't know.
fn known_context_by_name(provider_id: &str, model: &str) -> Option<usize> {
    let lower = model.trim().to_ascii_lowercase();
    // `openai/gpt-4o`, `@cf/meta/llama-3.1-8b-instruct`, ... -> the last part.
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let has = |s: &str| name.contains(s);
    let starts = |s: &str| name.starts_with(s);

    // Custom endpoints are usually a local server (Ollama defaults to a few
    // thousand tokens) whatever the model is called.
    if provider_id == "custom" {
        return Some(8_192);
    }
    if provider_id == CLOUDFLARE_WORKERS_AI_PROVIDER_ID || lower.contains("@cf/") {
        return Some(
            if has("llama-3.3-70b") || has("llama-4") || has("gpt-oss") {
                24_000
            } else if has("8b") || has("3b") || has("1b") || has("7b") {
                8_000
            } else {
                return None;
            },
        );
    }
    if starts("claude") {
        return Some(200_000);
    }
    if starts("gpt-4o")
        || starts("gpt-4.1")
        || starts("gpt-5")
        || starts("gpt-4-turbo")
        || starts("gpt-oss")
        || starts("o1")
        || starts("o3")
        || starts("o4")
        || starts("chatgpt-4o")
    {
        return Some(128_000);
    }
    if starts("gpt-4") {
        return Some(8_192);
    }
    if starts("gpt-3.5") {
        return Some(16_000);
    }
    if starts("gemini") || starts("grok") || starts("kimi") || starts("moonshot") {
        return Some(128_000);
    }
    if starts("deepseek") {
        return Some(64_000);
    }
    if starts("llama-3.1") || starts("llama-3.3") || starts("llama-4") || starts("llama3.1") {
        // Groq/Cerebras free tiers cap these well below 128k.
        return Some(32_000);
    }
    if starts("mistral") || starts("mixtral") || starts("qwen") || starts("command") {
        return Some(32_000);
    }
    None
}

impl LlmProvider for HttpProvider {
    fn name(&self) -> String {
        format!("{}/{}", self.provider.id, self.model)
    }

    fn context_tokens(&self) -> usize {
        if cached_context(&self.provider, &self.model).is_none()
            && lists_context_lengths(&self.provider)
            && !listing_backed_off(&self.provider)
        {
            // Once per provider: learn the context windows from the model list
            // (short timeout; a failure falls back to the name table and is
            // retried after `LISTING_RETRY_AFTER`).
            let provider = self.provider.clone();
            let api_key = self.api_key.clone();
            let listed = block_on(async move {
                tokio::time::timeout(Duration::from_secs(5), list_models(&provider, api_key)).await
            });
            let key = cache_key(&self.provider);
            match listed {
                Ok(Ok(_)) => {
                    // `list_models` cached the result, even one without metadata.
                    failed_listings().lock().remove(&key);
                }
                Ok(Err(e)) => {
                    log::debug!("{}: model list for context sizes failed: {e}", self.name());
                    failed_listings().lock().insert(key, Instant::now());
                }
                Err(_) => {
                    log::debug!("{}: model list for context sizes timed out", self.name());
                    failed_listings().lock().insert(key, Instant::now());
                }
            }
        }
        context_tokens_for(&self.provider, &self.model)
    }

    fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String> {
        self.send(self.params(messages, opts), opts)
    }

    fn chat_json(
        &self,
        messages: &[ChatMessage],
        json_schema: &Value,
        opts: &ChatOptions,
    ) -> Result<Value> {
        let endpoint = llm_client::endpoint_key(&self.provider, &self.model);
        if self.provider.supports_structured_output
            && self.provider.api_format == PostProcessApiFormat::OpenAi
            && !structured_output_rejections().lock().contains(&endpoint)
        {
            let mut params = self.params(messages, opts);
            params.json_schema = Some(("response".to_string(), json_schema.clone()));
            match self.send(params, opts) {
                Ok(reply) => {
                    // A reply that doesn't match the schema means the endpoint
                    // accepted `response_format` but didn't enforce it.
                    match extract_json(&reply)
                        .and_then(|v| validate_json(&v, json_schema).map(|_| v))
                    {
                        Ok(v) => return Ok(v),
                        Err(e) => log::warn!(
                            "{}: structured output reply was not valid ({e}); falling back to prompt-only JSON",
                            self.name()
                        ),
                    }
                }
                Err(e) if structured_output_rejected(&e) => {
                    log::warn!(
                        "{}: structured output rejected ({e}); using prompt-only JSON from now on",
                        self.name()
                    );
                    structured_output_rejections().lock().insert(endpoint);
                }
                Err(e) => return Err(e),
            }
        }
        // Prompt-only JSON retries once. A reply that still isn't valid JSON
        // is an `InvalidJsonReply`; auth, network and truncation errors from
        // `send` are returned unchanged.
        json_via_prompt(self, messages, json_schema, opts)
    }
}

/// List a provider's models. Cloudflare Workers AI has no OpenAI-style
/// `/models`, so it uses its model search API.
/// Also records the context windows the list reports (see
/// [`context_tokens_for`]). Refuses base URLs with `{placeholders}`.
pub async fn list_models(provider: &PostProcessProvider, api_key: String) -> Result<Vec<String>> {
    super::validate_base_url(provider)?;
    if provider.id == CLOUDFLARE_WORKERS_AI_PROVIDER_ID {
        return cloudflare_models(provider, &api_key).await;
    }
    let models = llm_client::fetch_models_with_info(provider, api_key)
        .await
        .map_err(|e| anyhow!(e))?;
    remember_model_info(provider, &models);
    Ok(models.into_iter().map(|m| m.id).collect())
}

/// `.../accounts/<id>/ai/v1` -> `.../accounts/<id>/ai/models/search`.
pub fn cloudflare_models_url(base_url: &str) -> Option<String> {
    let base = base_url.trim().trim_end_matches('/');
    let ai = base.strip_suffix("/v1")?;
    Some(format!(
        "{ai}/models/search?task=Text%20Generation&per_page=100"
    ))
}

async fn cloudflare_models(provider: &PostProcessProvider, api_key: &str) -> Result<Vec<String>> {
    let url = cloudflare_models_url(&provider.base_url)
        .ok_or_else(|| anyhow!("unexpected Cloudflare Workers AI base URL"))?;
    let response = reqwest::Client::new()
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| anyhow!("Cloudflare model list request failed: {}", e.without_url()))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Cloudflare model list failed ({status}): {body}"));
    }
    let json: Value = response
        .json()
        .await
        .map_err(|_| anyhow!("Cloudflare model list: unexpected response"))?;
    let models = parse_cloudflare_model_info(&json);
    remember_model_info(provider, &models);
    Ok(models.into_iter().map(|m| m.id).collect())
}

pub fn parse_cloudflare_models(json: &Value) -> Vec<String> {
    parse_cloudflare_model_info(json)
        .into_iter()
        .map(|m| m.id)
        .collect()
}

/// Model search results, with `properties: [{property_id: "context_window",
/// value: "24000"}]` when Cloudflare reports it.
pub fn parse_cloudflare_model_info(json: &Value) -> Vec<ModelInfo> {
    let context_of = |m: &Value| {
        m.get("properties")
            .and_then(Value::as_array)?
            .iter()
            .find(|p| p.get("property_id").and_then(Value::as_str) == Some("context_window"))
            .and_then(|p| p.get("value"))
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .filter(|&n| n >= 512)
            .map(|n| n as usize)
    };
    json.get("result")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    m.get("name").and_then(Value::as_str).map(|id| ModelInfo {
                        id: id.to_string(),
                        context_length: context_of(m),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn provider(id: &str, base_url: &str, structured: bool) -> PostProcessProvider {
        PostProcessProvider {
            id: id.into(),
            label: id.into(),
            base_url: base_url.into(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: structured,
            api_format: PostProcessApiFormat::OpenAi,
        }
    }

    /// Serve the given bodies (status 200 or the given status), one per
    /// connection, in order.
    async fn serve(responses: Vec<(&'static str, String)>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let _ = stream.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        });
        format!("http://{address}")
    }

    fn completion(content: &str) -> String {
        json!({"choices":[{"message":{"content":content},"finish_reason":"stop"}]}).to_string()
    }

    #[test]
    fn chat_strips_think_and_names_provider_model() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let base = rt.block_on(serve(vec![("200 OK", completion("<think>x</think>Hello"))]));
        let p = HttpProvider::new(provider("custom", &base, false), String::new(), "m".into());
        assert_eq!(p.name(), "custom/m");
        assert_eq!(p.context_tokens(), 8_192);
        assert_eq!(
            p.chat(&[ChatMessage::user("hi")], &ChatOptions::default())
                .unwrap(),
            "Hello"
        );
    }

    #[test]
    fn only_truncated_or_empty_messages_count_as_a_cut_short_probe() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let probe = |body: serde_json::Value| {
            let base = rt.block_on(serve(vec![("200 OK", body.to_string())]));
            let p = HttpProvider::new(provider("custom", &base, false), String::new(), "m".into());
            p.chat(&[ChatMessage::user("hi")], &ChatOptions::default())
                .unwrap_err()
        };
        let truncated =
            probe(json!({"choices":[{"message":{"content":""},"finish_reason":"length"}]}));
        assert!(super::super::is_probe_cut_short(&truncated));
        assert!(truncated.to_string().contains("truncated"), "{truncated}");

        let empty = probe(json!({"choices":[{"message":{"content":null},"finish_reason":"stop"}]}));
        assert!(super::super::is_probe_cut_short(&empty));
        assert_eq!(empty.to_string(), "custom returned an empty reply");

        // OpenRouter's upstream failure: a 200 with no content.
        let upstream_failed = probe(json!({"choices":[{
            "message":{"content":null},
            "finish_reason":"error",
            "error":{"code":502,"message":"Upstream provider timed out"}
        }]}));
        assert!(!super::super::is_probe_cut_short(&upstream_failed));
        assert!(
            upstream_failed.to_string().contains("'error'")
                && upstream_failed
                    .to_string()
                    .contains("Upstream provider timed out"),
            "{upstream_failed}"
        );
        let filtered =
            probe(json!({"choices":[{"message":{"content":""},"finish_reason":"content_filter"}]}));
        assert!(!super::super::is_probe_cut_short(&filtered));
        assert!(
            filtered.to_string().contains("content filter"),
            "{filtered}"
        );
        let no_reason =
            probe(json!({"choices":[{"message":{"content":null},"finish_reason":null}]}));
        assert!(!super::super::is_probe_cut_short(&no_reason));

        let no_choices = probe(json!({"choices":[]}));
        assert!(!super::super::is_probe_cut_short(&no_choices));
        assert_eq!(no_choices.to_string(), "custom returned an empty reply");

        let base = rt.block_on(serve(vec![("401 Unauthorized", "bad key".into())]));
        let p = HttpProvider::new(provider("custom", &base, false), String::new(), "m".into());
        let failed = p
            .chat(&[ChatMessage::user("hi")], &ChatOptions::default())
            .unwrap_err();
        assert!(!super::super::is_probe_cut_short(&failed));
    }

    #[test]
    fn chat_json_falls_back_to_prompt_when_structured_output_fails() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let base = rt.block_on(serve(vec![
            (
                "400 Bad Request",
                "{\"error\":\"response_format unsupported\"}".into(),
            ),
            ("200 OK", completion("```json\n{\"a\":\"b\"}\n```")),
        ]));
        let p = HttpProvider::new(provider("openai", &base, true), "k".into(), "m".into());
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        let v = p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .unwrap();
        assert_eq!(v, json!({"a":"b"}));
    }

    /// Like [`serve`], also returning each request's body as it arrives.
    async fn serve_recording(
        responses: Vec<(&'static str, String)>,
    ) -> (String, std::sync::Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = std::sync::Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 65536];
                // Read the headers, then the Content-Length body.
                loop {
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if n == 0 || buf.len() >= end + 4 + length {
                            recorded.lock().push(text[end + 4..].to_string());
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        });
        (format!("http://{address}"), requests)
    }

    #[test]
    fn a_rejected_structured_output_is_remembered_per_endpoint() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (base, requests) = rt.block_on(serve_recording(vec![
            (
                "400 Bad Request",
                json!({"error":{
                    "message":"Unsupported parameter.",
                    "param":"response_format",
                    "code":"unsupported_parameter"
                }})
                .to_string(),
            ),
            ("200 OK", completion("{\"a\":\"b\"}")),
            ("200 OK", completion("{\"a\":\"c\"}")),
        ]));
        let p = HttpProvider::new(provider("openai", &base, true), "k".into(), "m".into());
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        let first = p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .unwrap();
        assert_eq!(first, json!({"a":"b"}));
        // The next request skips the structured attempt entirely.
        let second = p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .unwrap();
        assert_eq!(second, json!({"a":"c"}));
        let requests = requests.lock().clone();
        let structured: Vec<bool> = requests
            .iter()
            .map(|body| body.contains("response_format"))
            .collect();
        assert_eq!(structured, vec![true, false, false], "{requests:?}");

        // Another model on the same endpoint still tries structured output.
        let other = llm_client::endpoint_key(&provider("openai", &base, true), "other");
        assert!(!structured_output_rejections().lock().contains(&other));
    }

    #[test]
    fn chat_json_falls_back_when_a_schema_reply_does_not_match() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let base = rt.block_on(serve(vec![
            ("200 OK", completion("not json")),
            ("200 OK", completion("{\"a\":\"b\"}")),
        ]));
        let p = HttpProvider::new(provider("openai", &base, true), "k".into(), "m".into());
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        let v = p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .unwrap();
        assert_eq!(v, json!({"a":"b"}));
    }

    #[test]
    fn chat_json_reports_an_unparseable_or_invalid_reply_as_invalid_json() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        // No structured output: json_via_prompt retries once, then the typed error.
        let prompt_only = rt.block_on(serve(vec![
            ("200 OK", completion("not json")),
            ("200 OK", completion(r#"{"a":1}"#)),
        ]));
        let p = HttpProvider::new(
            provider("custom", &prompt_only, false),
            String::new(),
            "m".into(),
        );
        let err = p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .unwrap_err();
        assert!(super::super::is_invalid_json_reply(&err), "{err:#}");
        assert!(err.to_string().contains("expected string"), "{err}");

        // Structured output accepts the call but doesn't return JSON. The
        // prompt-only fallback then fails both attempts.
        let structured = rt.block_on(serve(vec![
            ("200 OK", completion("not json")),
            ("200 OK", completion("still not")),
            ("200 OK", completion("nope")),
        ]));
        let p = HttpProvider::new(
            provider("openai", &structured, true),
            "k".into(),
            "m".into(),
        );
        let err = p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .unwrap_err();
        assert!(super::super::is_invalid_json_reply(&err), "{err:#}");
        assert!(err.to_string().contains("no JSON"), "{err}");
    }

    #[test]
    fn chat_json_propagates_failures_other_than_a_schema_rejection() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        let cases: Vec<(&'static str, String, &str)> = vec![
            (
                "401 Unauthorized",
                "{\"error\":\"invalid api key\"}".into(),
                "401",
            ),
            (
                "429 Too Many Requests",
                "{\"error\":\"rate limited\"}".into(),
                "429",
            ),
            (
                "500 Internal Server Error",
                "{\"error\":\"boom\"}".into(),
                "500",
            ),
            (
                "503 Service Unavailable",
                "schema service down".into(),
                "503",
            ),
            (
                "400 Bad Request",
                "{\"error\":\"This model's maximum context length is 8192 tokens (json_schema)\"}"
                    .into(),
                "maximum context length",
            ),
            (
                "200 OK",
                json!({"choices":[{"message":{"content":"{\"a\""},"finish_reason":"length"}]})
                    .to_string(),
                "truncated",
            ),
        ];
        for (status, body, expected) in cases {
            // A prompt-only retry would get this valid reply.
            let base = rt.block_on(serve(vec![
                (status, body),
                ("200 OK", completion("{\"a\":\"b\"}")),
            ]));
            let p = HttpProvider::new(provider("openai", &base, true), "k".into(), "m".into());
            let err = p
                .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
                .unwrap_err();
            assert!(err.to_string().contains(expected), "{status}: {err}");
            assert!(
                !super::super::is_invalid_json_reply(&err),
                "{status}: {err:#}"
            );
        }
        let truncated_base = rt.block_on(serve(vec![(
            "200 OK",
            json!({"choices":[{"message":{"content":""},"finish_reason":"length"}]}).to_string(),
        )]));
        let p = HttpProvider::new(
            provider("openai", &truncated_base, true),
            "k".into(),
            "m".into(),
        );
        let err = p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .unwrap_err();
        assert!(err.downcast_ref::<ReplyCutShort>().is_some(), "{err}");
        assert!(!super::super::is_invalid_json_reply(&err), "{err:#}");

        // Network failure: nothing listens on the port.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let p = HttpProvider::new(provider("openai", &base, true), "k".into(), "m".into());
        assert!(p
            .chat_json(&[ChatMessage::user("x")], &schema, &ChatOptions::default())
            .is_err());
    }

    #[test]
    fn only_a_rejected_response_format_counts_as_a_schema_rejection() {
        let status = |status: u16, body: &str| -> anyhow::Error {
            ApiStatusError {
                status,
                body: body.into(),
                message: String::new(),
            }
            .into()
        };
        assert!(structured_output_rejected(&status(
            400,
            "Invalid parameter: 'response_format' of type 'json_schema' is not supported"
        )));
        assert!(structured_output_rejected(&status(
            422,
            "structured outputs are not supported by this model"
        )));
        assert!(!structured_output_rejected(&status(
            400,
            "invalid model id"
        )));
        assert!(!structured_output_rejected(&status(401, "response_format")));
        assert!(!structured_output_rejected(&status(429, "json_schema")));
        assert!(!structured_output_rejected(&status(500, "json_schema")));
        assert!(!structured_output_rejected(&status(
            400,
            "context_length_exceeded: input plus json_schema too long"
        )));
        // OpenAI's typed error fields decide when present.
        assert!(structured_output_rejected(&status(
            400,
            r#"{"error":{"message":"Unsupported parameter.","type":"invalid_request_error","param":"response_format","code":"unsupported_parameter"}}"#
        )));
        assert!(structured_output_rejected(&status(
            400,
            r#"{"error":{"message":"Invalid value.","param":"text.format.schema","code":"invalid_value"}}"#
        )));
        assert!(structured_output_rejected(&status(
            400,
            r#"{"error":{"message":"Invalid schema for response_format 'response'.","code":"invalid_json_schema"}}"#
        )));
        assert!(!structured_output_rejected(&status(
            400,
            r#"{"error":{"message":"Unsupported parameter: 'temperature' (json_schema requests).","param":"temperature","code":"unsupported_parameter"}}"#
        )));
        // "schema" in an unrelated context isn't a structured-output rejection.
        assert!(!structured_output_rejected(&status(
            400,
            r#"{"error":{"message":"Invalid schema for function 'lookup': 'type' is required."}}"#
        )));
        assert!(!structured_output_rejected(&status(
            400,
            "database schema mismatch for this deployment"
        )));
        assert!(!structured_output_rejected(&anyhow!(
            "openai: response_format unsupported"
        )));
        assert!(!structured_output_rejected(
            &ReplyCutShort("schema truncated".into()).into()
        ));
    }

    #[test]
    fn cancel_flag_stops_a_pending_request() {
        // A server that accepts but never answers.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        rt.spawn(async move {
            let (_s, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let p = HttpProvider::new(provider("custom", &base, false), String::new(), "m".into());
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            c2.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let err = p
            .chat(
                &[ChatMessage::user("hi")],
                &ChatOptions {
                    cancel: Some(cancel),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(super::super::is_cancelled_error(&err));
    }

    #[test]
    fn cloudflare_model_listing_url_and_parsing() {
        assert_eq!(
            cloudflare_models_url("https://api.cloudflare.com/client/v4/accounts/abc/ai/v1/")
                .unwrap(),
            "https://api.cloudflare.com/client/v4/accounts/abc/ai/models/search?task=Text%20Generation&per_page=100"
        );
        let json =
            json!({"success":true,"result":[{"name":"@cf/meta/llama-3.1-8b-instruct"},{"id":"x"}]});
        assert_eq!(
            parse_cloudflare_models(&json),
            vec!["@cf/meta/llama-3.1-8b-instruct".to_string()]
        );
    }

    #[test]
    fn context_comes_from_the_model_name_when_nothing_is_listed() {
        assert_eq!(context_tokens_by_name("openai", "gpt-4o-mini"), 128_000);
        assert_eq!(
            context_tokens_by_name("openrouter", "openai/gpt-5"),
            128_000
        );
        assert_eq!(context_tokens_by_name("openai", "gpt-4.1-nano"), 128_000);
        assert_eq!(
            context_tokens_by_name("anthropic", "claude-haiku-4-5"),
            200_000
        );
        assert_eq!(
            context_tokens_by_name("vercel_ai_gateway", "anthropic/claude-sonnet-4"),
            200_000
        );
        assert_eq!(
            context_tokens_by_name(
                CLOUDFLARE_WORKERS_AI_PROVIDER_ID,
                "@cf/meta/llama-3.1-8b-instruct"
            ),
            8_000
        );
        assert_eq!(
            context_tokens_by_name(
                CLOUDFLARE_WORKERS_AI_PROVIDER_ID,
                "@cf/meta/llama-3.3-70b-instruct-fp8-fast"
            ),
            24_000
        );
        // Through the AI Gateway, Workers AI models keep their small windows.
        assert_eq!(
            context_tokens_by_name("cloudflare_ai_gateway", "@cf/meta/llama-3.2-3b-instruct"),
            8_000
        );
        assert_eq!(context_tokens_by_name("custom", "gpt-4o"), 8_192);
        assert_eq!(
            context_tokens_by_name("groq", "some-new-model"),
            DEFAULT_CONTEXT_TOKENS
        );
    }

    #[test]
    fn listed_context_lengths_win_and_are_cached_per_provider_and_model() {
        let p = provider("openrouter", "https://ctx-test.invalid/api/v1", true);
        let other = provider("openrouter", "https://ctx-test-other.invalid/api/v1", true);
        remember_model_info(
            &p,
            &[
                ModelInfo {
                    id: "meta-llama/llama-3.1-8b-test".into(),
                    context_length: Some(131_072),
                },
                ModelInfo {
                    id: "x/unlisted".into(),
                    context_length: None,
                },
            ],
        );
        assert_eq!(
            context_tokens_for(&p, "meta-llama/llama-3.1-8b-test"),
            131_072
        );
        // Not reported: the name table.
        assert_eq!(context_tokens_for(&p, "x/unlisted"), DEFAULT_CONTEXT_TOKENS);
        // Another base URL is another provider.
        assert_eq!(
            context_tokens_for(&other, "meta-llama/llama-3.1-8b-test"),
            32_000
        );
        // Already listed: no request (the URL is unroutable).
        let http = HttpProvider::new(p, "k".into(), "meta-llama/llama-3.1-8b-test".into());
        assert_eq!(http.context_tokens(), 131_072);
    }

    #[test]
    fn registry_context_fills_in_for_the_name_table_but_not_the_listing() {
        let reg = super::super::registry::current();
        // A model the name table doesn't know: the registry's size.
        let (id, listed) = reg
            .models("openrouter")
            .unwrap()
            .iter()
            .filter(|m| known_context_by_name("openrouter", &m.id).is_none())
            .find_map(|m| Some((m.id.clone(), m.limit.context?)))
            .unwrap();
        let p = provider("openrouter", "https://ctx-registry.invalid/api/v1", true);
        assert_eq!(context_tokens_for(&p, &id), listed as usize);
        // One it knows: its hosted size, not the registry's maximum.
        let (known, n) = reg
            .models("openrouter")
            .unwrap()
            .iter()
            .filter(|m| m.limit.context.is_some())
            .find_map(|m| Some((m.id.clone(), known_context_by_name("openrouter", &m.id)?)))
            .unwrap();
        assert_eq!(context_tokens_for(&p, &known), n);
        remember_model_info(
            &p,
            &[ModelInfo {
                id: id.clone(),
                context_length: Some(12_345),
            }],
        );
        assert_eq!(context_tokens_for(&p, &id), 12_345);
    }

    #[test]
    fn openrouter_context_is_learned_from_one_model_listing() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let base = rt.block_on(serve(vec![(
            "200 OK",
            json!({"data":[{"id":"z-ai/glm-test","context_length":202752}]}).to_string(),
        )]));
        let p = HttpProvider::new(
            provider("openrouter", &base, true),
            "k".into(),
            "z-ai/glm-test".into(),
        );
        assert_eq!(p.context_tokens(), 202_752);
        // Cached: the mock serves only one connection.
        assert_eq!(p.context_tokens(), 202_752);
    }

    #[test]
    fn a_failed_listing_is_retried_after_a_backoff_not_cached() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let base = rt.block_on(serve(vec![
            ("500 Internal Server Error", "down".into()),
            (
                "200 OK",
                json!({"data":[{"id":"z-ai/glm-test","context_length":202752}]}).to_string(),
            ),
        ]));
        let p = HttpProvider::new(
            provider("openrouter", &base, true),
            "k".into(),
            "z-ai/glm-test".into(),
        );
        assert_eq!(p.context_tokens(), DEFAULT_CONTEXT_TOKENS);
        assert_eq!(cached_context(&p.provider, &p.model), None);
        // Within the backoff: no request (the next one would succeed).
        assert_eq!(p.context_tokens(), DEFAULT_CONTEXT_TOKENS);
        // Once the backoff has passed, the listing is tried again.
        let long_ago = Instant::now()
            .checked_sub(LISTING_RETRY_AFTER + Duration::from_secs(1))
            .unwrap();
        failed_listings()
            .lock()
            .insert(cache_key(&p.provider), long_ago);
        assert_eq!(p.context_tokens(), 202_752);
        assert!(!failed_listings()
            .lock()
            .contains_key(&cache_key(&p.provider)));
    }

    #[test]
    fn cloudflare_listing_reports_context_windows() {
        let json = json!({"result":[{
            "name":"@cf/meta/llama-3.1-8b-instruct",
            "properties":[{"property_id":"context_window","value":"7968"}]
        }]});
        assert_eq!(
            parse_cloudflare_model_info(&json),
            vec![ModelInfo {
                id: "@cf/meta/llama-3.1-8b-instruct".into(),
                context_length: Some(7_968),
            }]
        );
    }

    #[test]
    fn list_models_refuses_placeholder_urls() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let p = provider(
            CLOUDFLARE_WORKERS_AI_PROVIDER_ID,
            "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1",
            false,
        );
        let err = rt
            .block_on(list_models(&p, "k".into()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("{account_id}"), "{err}");
    }

    #[test]
    fn list_models_reads_openai_style_lists() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let base = rt.block_on(serve(vec![(
            "200 OK",
            json!({"data":[{"id":"a"},{"id":"b"}]}).to_string(),
        )]));
        let models = rt
            .block_on(list_models(
                &provider("vercel_ai_gateway", &base, true),
                "k".into(),
            ))
            .unwrap();
        assert_eq!(models, vec!["a", "b"]);
    }
}
