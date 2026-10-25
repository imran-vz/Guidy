//! LLM provider layer (plan task 2.1 / task 4). See docs/MEETING_NOTES_ARCH.md.
//!
//! One [`LlmProvider`] interface over chat completion and structured JSON
//! output, with three kinds of backends:
//! - [`http::HttpProvider`]: every `post_process_providers` entry (OpenAI-
//!   compatible and Anthropic formats, incl. the BYOK gateways), through
//!   [`crate::llm_client`].
//! - [`apple::AppleProvider`]: Apple Intelligence (macOS, Apple silicon).
//! - [`builtin::BuiltinProvider`]: the built-in llama.cpp worker
//!   (`guidy-llm`), with models from [`local_models`].
//!
//! Model choice is per feature ([`Feature`]): dictation post-processing uses
//! `post_process_provider_id` / `post_process_models`, meetings use
//! `meetings.llm_provider_id` / `meetings.llm_models`.
//!
//! The trait is blocking: summaries run on a background thread. Don't call it
//! from a current-thread async runtime's own thread.

pub mod apple;
pub mod builtin;
pub mod http;
pub mod local_models;
pub mod registry;
pub mod usage;

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::host::AppHandle;
use crate::meetings::settings::BUILTIN_LLM_PROVIDER_ID;
use crate::settings::{AppSettings, PostProcessProvider, APPLE_INTELLIGENCE_PROVIDER_ID};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Shared cancel flag: set it to stop a running request.
pub type CancelFlag = Arc<AtomicBool>;

#[derive(Debug, Clone, Default)]
pub struct ChatOptions {
    /// Output budget. Cloud OpenAI-format endpoints use their own default
    /// unless this is set.
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub cancel: Option<CancelFlag>,
    /// Built-in runtime only: how long to wait for its single worker while
    /// another request (e.g. a minutes-long meeting summary) uses it, before
    /// failing with [`builtin::WorkerBusy`]. `None` waits until it's free.
    /// Bounded waiters also go ahead of unbounded ones queued for the worker.
    pub max_queue_wait: Option<std::time::Duration>,
    /// Cloud providers: totals for this job's calls (see [`usage`]). Every
    /// cloud call is recorded in the usage ledger either way.
    pub meter: Option<usage::UsageMeter>,
}

impl ChatOptions {
    pub fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
    }
}

/// Error text used when a request was cancelled through [`ChatOptions::cancel`].
pub const CANCELLED: &str = "cancelled";

pub fn is_cancelled_error(e: &anyhow::Error) -> bool {
    e.to_string() == CANCELLED
}

/// The provider's final reply was not JSON matching the schema, after its
/// own retries. A reply cut off at the output token limit counts: the text
/// never became valid JSON. Find it with [`is_invalid_json_reply`].
#[derive(Debug)]
pub struct InvalidJsonReply {
    message: String,
}

impl InvalidJsonReply {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for InvalidJsonReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InvalidJsonReply {}

/// Whether `e` (or anything it wraps) is an [`InvalidJsonReply`].
pub fn is_invalid_json_reply(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| cause.is::<InvalidJsonReply>())
}

fn invalid_json(error: anyhow::Error) -> anyhow::Error {
    InvalidJsonReply::new(error.to_string()).into()
}

/// A chat model behind one interface.
pub trait LlmProvider: Send + Sync {
    /// `"<provider_id>/<model>"`, stored as `StoredSummary::generated_by`.
    fn name(&self) -> String;

    /// Usable context window in tokens (prompt + output).
    fn context_tokens(&self) -> usize;

    /// Plain chat completion. Returns the reply text (any leading
    /// `<think>` block removed).
    fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String>;

    /// Chat with the reply constrained to `json_schema`. The default asks for
    /// JSON in the prompt, then validates and retries once with the error.
    /// A reply that still isn't valid JSON is an [`InvalidJsonReply`].
    fn chat_json(
        &self,
        messages: &[ChatMessage],
        json_schema: &Value,
        opts: &ChatOptions,
    ) -> Result<Value> {
        json_via_prompt(self, messages, json_schema, opts)
    }
}

/// Which feature a model is chosen for (plan: per-feature model choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    PostProcess,
    Meetings,
}

/// The `(provider_id, model)` a feature uses. Meetings fall back to the
/// provider's post-processing model when no meetings model is set.
pub fn resolve_provider_model(
    settings: &AppSettings,
    feature: Feature,
    provider_override: Option<(String, String)>,
) -> (String, String) {
    if let Some((provider, model)) = provider_override {
        return (provider, model);
    }
    let id = match feature {
        Feature::PostProcess => settings.post_process_provider_id.clone(),
        Feature::Meetings => settings.meetings.llm_provider_id.clone(),
    };
    let model = feature_model(settings, feature, &id);
    (id, model)
}

/// The model `feature` uses on `provider_id` (empty when none is set).
/// Meetings fall back to the provider's post-processing model.
pub fn feature_model(settings: &AppSettings, feature: Feature, provider_id: &str) -> String {
    let post_process = settings.post_process_models.get(provider_id);
    match feature {
        Feature::PostProcess => post_process,
        Feature::Meetings => settings
            .meetings
            .llm_models
            .get(provider_id)
            .filter(|m| !m.trim().is_empty())
            .or(post_process),
    }
    .cloned()
    .unwrap_or_default()
}

/// A provider id resolved against the settings: the built-in llama.cpp
/// runtime, which isn't a `post_process_providers` entry, or a configured
/// provider. Every lookup by id goes through [`resolve_provider`] so no call
/// site special-cases the built-in id.
#[derive(Debug, Clone, Copy)]
pub enum ProviderRef<'a> {
    /// The built-in llama.cpp runtime ([`BUILTIN_LLM_PROVIDER_ID`]).
    Builtin,
    /// A `post_process_providers` entry (incl. Apple Intelligence).
    Configured(&'a PostProcessProvider),
}

impl<'a> ProviderRef<'a> {
    /// The `post_process_providers` entry, None for the built-in runtime.
    pub fn configured(self) -> Option<&'a PostProcessProvider> {
        match self {
            Self::Builtin => None,
            Self::Configured(p) => Some(p),
        }
    }
}

/// The provider `provider_id` names, or None when it names none.
pub fn resolve_provider<'a>(
    settings: &'a AppSettings,
    provider_id: &str,
) -> Option<ProviderRef<'a>> {
    if is_builtin_provider(provider_id) {
        Some(ProviderRef::Builtin)
    } else {
        settings
            .post_process_provider(provider_id)
            .map(ProviderRef::Configured)
    }
}

/// Whether `provider_id` is the built-in llama.cpp runtime. For callers that
/// only have an id; with settings at hand, use [`resolve_provider`].
pub fn is_builtin_provider(provider_id: &str) -> bool {
    provider_id == BUILTIN_LLM_PROVIDER_ID
}

/// Build the provider for a feature (or an explicit `(provider_id, model)`).
pub fn provider_for_feature(
    app: &AppHandle,
    settings: &AppSettings,
    feature: Feature,
    provider_override: Option<(String, String)>,
) -> Result<Arc<dyn LlmProvider>> {
    let (provider_id, model) = resolve_provider_model(settings, feature, provider_override);
    provider_by_id(app, settings, &provider_id, &model)
}

/// Build a provider by id. `model` may be empty for the built-in runtime
/// (= the default downloaded model) and Apple Intelligence.
pub fn provider_by_id(
    app: &AppHandle,
    settings: &AppSettings,
    provider_id: &str,
    model: &str,
) -> Result<Arc<dyn LlmProvider>> {
    if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        return Ok(Arc::new(apple::AppleProvider::new()?));
    }
    let provider = match resolve_provider(settings, provider_id) {
        Some(ProviderRef::Builtin) => {
            return Ok(Arc::new(builtin::BuiltinProvider::new(app, model)?));
        }
        Some(ProviderRef::Configured(p)) => p.clone(),
        None => bail!("unknown LLM provider '{provider_id}'"),
    };
    validate_base_url(&provider)?;
    if model.trim().is_empty() {
        bail!("no model selected for {}", provider.label);
    }
    let api_key = crate::settings::provider_api_key(provider_id);
    if api_key.trim().is_empty() && provider.id != "custom" {
        bail!("no API key set for {}", provider.label);
    }
    Ok(Arc::new(http::HttpProvider::new(
        provider,
        api_key,
        model.trim().to_string(),
    )))
}

/// Providers whose URL needs user input (Cloudflare's account / gateway id)
/// ship with `{placeholders}`; refuse to call them until filled in. Any `{`
/// or `}` is refused (a real URL never has one unescaped). The one check for
/// every caller (meetings LLM layer and dictation post-processing).
pub fn validate_base_url(provider: &PostProcessProvider) -> Result<()> {
    let url = provider.base_url.trim();
    if url.is_empty() {
        bail!("{} has no base URL", provider.label);
    }
    if let Some(a) = url.find(['{', '}']) {
        let placeholder = url[a..]
            .strip_prefix('{')
            .and_then(|rest| rest.find(['{', '}']))
            .map(|b| &url[a..=a + 1 + b])
            .filter(|p| p.ends_with('}'));
        match placeholder {
            Some(p) => bail!(
                "Set the {p} part of the {} base URL in the provider settings",
                provider.label
            ),
            None => bail!(
                "The {} base URL has a stray '{{' or '}}': fix it in the provider settings",
                provider.label
            ),
        }
    }
    Ok(())
}

/// Check that a provider is usable. Returns a short human-readable status
/// for the "Test connection" button; the error text explains what's wrong.
/// `feature` picks the model probed when the provider has no model list (the
/// post-processing page tests the post-processing model, the meetings AI
/// settings the meetings model). Blocking. Never logs or returns the key.
pub fn test_connection(app: &AppHandle, provider_id: &str, feature: Feature) -> Result<String> {
    let settings = crate::settings::get_settings(app);
    if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        apple::AppleProvider::new()?;
        return Ok("Apple Intelligence is available".to_string());
    }
    let provider = match resolve_provider(&settings, provider_id) {
        Some(ProviderRef::Builtin) => {
            let model = feature_model(&settings, feature, provider_id);
            let provider = builtin::BuiltinProvider::new(app, &model)?;
            return Ok(match builtin::BuiltinLlm::global().ping()? {
                builtin::PingStatus::Ready => {
                    format!("Built-in model ready: {}", provider.model_label())
                }
                builtin::PingStatus::Busy => format!(
                    "Built-in model is busy generating; it is running ({})",
                    provider.model_label()
                ),
            });
        }
        Some(ProviderRef::Configured(p)) => p.clone(),
        None => bail!("unknown LLM provider '{provider_id}'"),
    };
    validate_base_url(&provider)?;
    let api_key = crate::settings::provider_api_key(provider_id);
    if api_key.trim().is_empty() && provider.id != "custom" {
        bail!("no API key set for {}", provider.label);
    }
    match block_on(http::list_models(&provider, api_key.clone())) {
        Ok(models) if !models.is_empty() => Ok(format!(
            "Connected to {} ({} models available)",
            provider.label,
            models.len()
        )),
        listing => {
            // Some endpoints have no model list: fall back to a 1-token chat
            // with the configured model.
            let model = feature_model(&settings, feature, provider_id);
            if model.trim().is_empty() {
                return match listing {
                    Ok(_) => Ok(format!(
                        "Connected to {} (no models listed)",
                        provider.label
                    )),
                    Err(e) => Err(e),
                };
            }
            let http = http::HttpProvider::new(provider.clone(), api_key, model.clone());
            let reply = http.chat(
                &[ChatMessage::user("Reply with OK.")],
                &ChatOptions {
                    max_tokens: Some(1),
                    ..Default::default()
                },
            );
            match reply {
                Ok(_) => Ok(format!("Connected to {} ({model} replied)", provider.label)),
                // A 1-token budget is often spent before any text (reasoning
                // models): the request still went through.
                Err(e) if is_probe_cut_short(&e) => Ok(format!(
                    "Connected to {} ({model} answered)",
                    provider.label
                )),
                Err(e) => Err(e),
            }
        }
    }
}

/// Whether a probe's error only says the 1-token reply was cut off or had an
/// empty message (not a response without any choice).
fn is_probe_cut_short(e: &anyhow::Error) -> bool {
    e.downcast_ref::<http::ReplyCutShort>().is_some()
}

/// Model ids a provider offers (the built-in runtime: downloaded GGUFs).
/// Blocking.
pub fn list_models(app: &AppHandle, provider_id: &str) -> Result<Vec<String>> {
    if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        return Ok(vec![
            crate::settings::APPLE_INTELLIGENCE_DEFAULT_MODEL_ID.to_string()
        ]);
    }
    let settings = crate::settings::get_settings(app);
    let provider = match resolve_provider(&settings, provider_id) {
        Some(ProviderRef::Builtin) => {
            return Ok(local_models::LocalModels::new(app)?
                .list()
                .into_iter()
                .filter(|m| m.downloaded)
                .map(|m| m.id)
                .collect());
        }
        Some(ProviderRef::Configured(p)) => p.clone(),
        None => bail!("unknown LLM provider '{provider_id}'"),
    };
    validate_base_url(&provider)?;
    let api_key = crate::settings::provider_api_key(provider_id);
    block_on(http::list_models(&provider, api_key))
}

/// The model picker's list for a cloud provider ([`registry::merge_models`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ModelCatalog {
    pub models: Vec<registry::ModelEntry>,
    /// models.dev describes this provider.
    pub in_registry: bool,
    /// When the registry copy was downloaded (unix seconds); `None` for the
    /// copy bundled with the app.
    pub registry_fetched_at: Option<u64>,
    /// Why the provider's own model list couldn't be read (no API key, a
    /// network error). The registry's models are shown meanwhile.
    pub live_error: Option<String>,
}

/// The models a cloud provider offers, described by the registry. The live
/// list is only asked for once an API key is set (or for the custom
/// endpoint, which may need none). Blocking.
pub fn model_catalog(app: &AppHandle, provider_id: &str) -> Result<ModelCatalog> {
    let settings = crate::settings::get_settings(app);
    let provider = match resolve_provider(&settings, provider_id) {
        Some(ProviderRef::Configured(p)) => p.clone(),
        Some(ProviderRef::Builtin) => bail!("the built-in runtime has no model catalog"),
        None => bail!("unknown LLM provider '{provider_id}'"),
    };
    let api_key = crate::settings::provider_api_key(provider_id);
    let live = if api_key.trim().is_empty() && provider.id != "custom" {
        Err(anyhow!("no API key set for {}", provider.label))
    } else {
        validate_base_url(&provider).and_then(|_| block_on(http::list_models(&provider, api_key)))
    };
    let reg = registry::current();
    let known = reg.models(provider_id);
    let (live, live_error) = match live {
        Ok(ids) => (Some(ids), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    Ok(ModelCatalog {
        models: registry::merge_models(known, live.as_deref()),
        in_registry: known.is_some(),
        registry_fetched_at: reg.fetched_at,
        live_error,
    })
}

/// Run a future to completion from blocking code, whether or not the caller
/// is already inside a tokio runtime.
pub(crate) fn block_on<F>(future: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(future))
        }
        // A current-thread runtime can't be blocked from its own thread: run
        // the future on the shared runtime from a helper thread.
        Ok(_) => std::thread::scope(|s| {
            s.spawn(|| crate::host::async_runtime::block_on(future))
                .join()
                .expect("LLM request thread panicked")
        }),
        Err(_) => crate::host::async_runtime::block_on(future),
    }
}

// ----- JSON helpers -------------------------------------------------------

/// Remove a leading `<think>...</think>` block (reasoning models).
pub fn strip_think(s: &str) -> &str {
    let t = s.trim_start();
    if let Some(rest) = t.strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + "</think>".len()..].trim_start();
        }
    }
    s
}

/// Parse the JSON object in a model reply, tolerating think blocks, code
/// fences and prose around it.
pub fn extract_json(reply: &str) -> Result<Value> {
    let text = strip_think(reply).trim();
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return Ok(v);
    }
    let start = text
        .find('{')
        .ok_or_else(|| anyhow!("the reply contains no JSON object"))?;
    let end = text
        .rfind('}')
        .filter(|&e| e > start)
        .ok_or_else(|| anyhow!("the reply's JSON object is incomplete"))?;
    serde_json::from_str(&text[start..=end]).map_err(|e| anyhow!("invalid JSON in the reply: {e}"))
}

/// Check `value` against the subset of JSON Schema we generate: `type`
/// (incl. `["string","null"]`), `required`, `properties`, `items`, `enum`.
pub fn validate_json(value: &Value, schema: &Value) -> Result<()> {
    validate_at(value, schema, "$")
}

fn validate_at(value: &Value, schema: &Value, path: &str) -> Result<()> {
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let ok = types.is_empty()
            || types.iter().any(|t| match *t {
                "object" => value.is_object(),
                "array" => value.is_array(),
                "string" => value.is_string(),
                "number" => value.is_number(),
                "integer" => value.is_i64() || value.is_u64(),
                "boolean" => value.is_boolean(),
                "null" => value.is_null(),
                _ => true,
            });
        if !ok {
            bail!("{path}: expected {}", types.join(" or "));
        }
    }
    if let (Some(allowed), false) = (
        schema.get("enum").and_then(Value::as_array),
        value.is_null(),
    ) {
        if !allowed.contains(value) {
            bail!("{path}: value not allowed");
        }
    }
    if let Value::Object(map) = value {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !map.contains_key(key) {
                    bail!("{path}: missing \"{key}\"");
                }
            }
        }
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (key, sub) in props {
                if let Some(v) = map.get(key) {
                    validate_at(v, sub, &format!("{path}.{key}"))?;
                }
            }
        }
    }
    if let (Value::Array(items), Some(item_schema)) = (value, schema.get("items")) {
        for (i, v) in items.iter().enumerate() {
            validate_at(v, item_schema, &format!("{path}[{i}]"))?;
        }
    }
    Ok(())
}

/// The system-prompt suffix used when a provider can't enforce a schema.
pub fn json_instruction(json_schema: &Value) -> String {
    format!(
        "Respond with a single JSON object only: no prose, no code fences. It must match this JSON schema:\n{}",
        serde_json::to_string(json_schema).unwrap_or_default()
    )
}

/// `chat_json` for providers without native structured output: ask for JSON
/// in the system prompt, validate, and retry once with the validation error.
/// The final failure is an [`InvalidJsonReply`]. A `chat` error (auth,
/// network, cancel) is returned as-is and is not retried.
pub fn json_via_prompt<P: LlmProvider + ?Sized>(
    provider: &P,
    messages: &[ChatMessage],
    json_schema: &Value,
    opts: &ChatOptions,
) -> Result<Value> {
    let mut msgs = with_json_instruction(messages, json_schema);
    let mut last_err = None;
    for _attempt in 0..2 {
        let reply = provider.chat(&msgs, opts)?;
        match extract_json(&reply).and_then(|v| validate_json(&v, json_schema).map(|_| v)) {
            Ok(v) => return Ok(v),
            Err(e) => {
                log::warn!(
                    "{}: reply was not valid JSON ({e}); retrying",
                    provider.name()
                );
                msgs.push(ChatMessage::assistant(reply));
                msgs.push(ChatMessage::user(format!(
                    "That reply was not valid: {e}. Reply again with only the corrected JSON object."
                )));
                last_err = Some(e);
            }
        }
    }
    Err(invalid_json(
        last_err.unwrap_or_else(|| anyhow!("no valid JSON reply")),
    ))
}

/// Append the JSON instruction to the system message (or add one).
pub fn with_json_instruction(messages: &[ChatMessage], json_schema: &Value) -> Vec<ChatMessage> {
    let instruction = json_instruction(json_schema);
    let mut msgs = messages.to_vec();
    match msgs.iter_mut().find(|m| m.role == Role::System) {
        Some(system) => {
            system.content.push_str("\n\n");
            system.content.push_str(&instruction);
        }
        None => msgs.insert(0, ChatMessage::system(instruction)),
    }
    msgs
}

/// Rough token estimate (about 4 characters per token).
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use serde_json::json;

    #[test]
    fn extract_json_tolerates_fences_think_and_prose() {
        assert_eq!(extract_json(r#"{"a":1}"#).unwrap(), json!({"a":1}));
        assert_eq!(
            extract_json("<think>hmm {not json}</think>\n```json\n{\"a\": [1]}\n```").unwrap(),
            json!({"a":[1]})
        );
        assert_eq!(
            extract_json("Sure! Here it is: {\"b\": \"}\"} hope that helps").unwrap(),
            json!({"b":"}"})
        );
        assert!(extract_json("no json here").is_err());
    }

    #[test]
    fn validate_json_checks_types_required_and_items() {
        let schema = json!({
            "type": "object",
            "required": ["a", "b"],
            "properties": {
                "a": {"type": "string"},
                "b": {"type": "array", "items": {"type": ["string", "null"]}}
            }
        });
        assert!(validate_json(&json!({"a": "x", "b": ["y", null]}), &schema).is_ok());
        assert!(validate_json(&json!({"a": "x"}), &schema)
            .unwrap_err()
            .to_string()
            .contains("missing \"b\""));
        assert!(validate_json(&json!({"a": 1, "b": []}), &schema).is_err());
        assert!(validate_json(&json!({"a": "x", "b": [3]}), &schema).is_err());
    }

    #[test]
    fn validate_base_url_rejects_placeholders() {
        let mut p = PostProcessProvider {
            id: "cloudflare_workers_ai".into(),
            label: "Cloudflare Workers AI".into(),
            base_url: "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1".into(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: Default::default(),
        };
        let err = validate_base_url(&p).unwrap_err().to_string();
        assert!(err.contains("{account_id}"), "{err}");
        p.base_url = "https://api.cloudflare.com/client/v4/accounts/abc123/ai/v1".into();
        assert!(validate_base_url(&p).is_ok());
        // Any brace is refused, including a `}` before a `{`.
        for url in [
            "https://x.dev/}a{/v1",
            "https://x.dev/{/v1",
            "https://x.dev/}/v1",
            "https://x.dev/{a{b}/v1",
        ] {
            p.base_url = url.into();
            let err = validate_base_url(&p).unwrap_err().to_string();
            assert!(err.contains("Cloudflare Workers AI"), "{url}: {err}");
        }
        p.base_url = "https://x.dev/{a{b}/v1".into();
        assert!(validate_base_url(&p)
            .unwrap_err()
            .to_string()
            .contains("stray"));
    }

    #[test]
    fn resolve_uses_per_feature_settings_and_override() {
        let mut s = crate::settings::get_default_settings();
        s.post_process_provider_id = "openai".into();
        s.post_process_models
            .insert("openai".into(), "gpt-pp".into());
        s.meetings.llm_provider_id = "openai".into();
        assert_eq!(
            resolve_provider_model(&s, Feature::PostProcess, None),
            ("openai".into(), "gpt-pp".into())
        );
        // Meetings fall back to the provider's post-processing model...
        assert_eq!(
            resolve_provider_model(&s, Feature::Meetings, None).1,
            "gpt-pp"
        );
        // ...until a meetings model is chosen.
        s.meetings
            .llm_models
            .insert("openai".into(), "gpt-meet".into());
        assert_eq!(
            resolve_provider_model(&s, Feature::Meetings, None).1,
            "gpt-meet"
        );
        assert_eq!(
            resolve_provider_model(&s, Feature::Meetings, Some(("x".into(), "y".into()))),
            ("x".into(), "y".into())
        );
    }

    #[test]
    fn provider_ids_resolve_to_the_builtin_runtime_or_a_configured_provider() {
        let s = crate::settings::get_default_settings();
        assert!(matches!(
            resolve_provider(&s, BUILTIN_LLM_PROVIDER_ID),
            Some(ProviderRef::Builtin)
        ));
        assert!(is_builtin_provider(BUILTIN_LLM_PROVIDER_ID));
        assert!(!is_builtin_provider("openai"));
        match resolve_provider(&s, "openai") {
            Some(ProviderRef::Configured(p)) => assert_eq!(p.id, "openai"),
            other => panic!("expected the configured provider, got {other:?}"),
        }
        assert!(resolve_provider(&s, "no_such_provider").is_none());
    }

    #[test]
    fn feature_model_picks_the_callers_model() {
        let mut s = crate::settings::get_default_settings();
        s.post_process_models
            .insert("openai".into(), "gpt-pp".into());
        s.meetings
            .llm_models
            .insert("openai".into(), "gpt-meet".into());
        assert_eq!(feature_model(&s, Feature::PostProcess, "openai"), "gpt-pp");
        assert_eq!(feature_model(&s, Feature::Meetings, "openai"), "gpt-meet");
        s.meetings.llm_models.insert("openai".into(), " ".into());
        assert_eq!(feature_model(&s, Feature::Meetings, "openai"), "gpt-pp");
        assert_eq!(feature_model(&s, Feature::PostProcess, "groq"), "");
    }

    #[test]
    fn a_cut_short_probe_is_recognised_by_type_not_text() {
        // Same text as a cut-short reply, but not the typed error.
        assert!(!is_probe_cut_short(&anyhow!(
            "OpenAI: Response was truncated at the provider's output token limit"
        )));
        assert!(!is_probe_cut_short(&anyhow!(
            "OpenAI returned an empty reply"
        )));
        // End-to-end cases are in `http::tests`.
    }

    /// Replies from a script, recording what it was sent.
    struct Scripted {
        replies: Mutex<Vec<String>>,
        seen: Mutex<Vec<Vec<ChatMessage>>>,
    }

    impl LlmProvider for Scripted {
        fn name(&self) -> String {
            "fake/scripted".into()
        }
        fn context_tokens(&self) -> usize {
            4096
        }
        fn chat(&self, messages: &[ChatMessage], _: &ChatOptions) -> Result<String> {
            self.seen.lock().push(messages.to_vec());
            Ok(self.replies.lock().remove(0))
        }
    }

    #[test]
    fn json_via_prompt_retries_once_with_the_error() {
        let p = Scripted {
            replies: Mutex::new(vec!["not json".into(), r#"{"a":"ok"}"#.into()]),
            seen: Mutex::new(vec![]),
        };
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        let v = p
            .chat_json(&[ChatMessage::user("hi")], &schema, &ChatOptions::default())
            .unwrap();
        assert_eq!(v, json!({"a":"ok"}));
        let seen = p.seen.lock();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0][0].role, Role::System);
        assert!(seen[0][0].content.contains("JSON schema"));
        assert!(seen[1].last().unwrap().content.contains("not valid"));
    }

    #[test]
    fn json_via_prompt_returns_invalid_json_reply_after_retrying() {
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        let unparseable = Scripted {
            replies: Mutex::new(vec!["not json".into(), "still no".into()]),
            seen: Mutex::new(vec![]),
        };
        let err = unparseable
            .chat_json(&[ChatMessage::user("hi")], &schema, &ChatOptions::default())
            .unwrap_err();
        assert!(is_invalid_json_reply(&err), "{err:#}");
        assert!(err.to_string().contains("no JSON"), "{err}");
        assert_eq!(unparseable.seen.lock().len(), 2);
        // The same words, wrapped later, still count. A plain error with
        // that text does not.
        let wrapped = anyhow::Error::from(InvalidJsonReply::new(err.to_string())).context("later");
        assert!(is_invalid_json_reply(&wrapped));
        assert!(!is_invalid_json_reply(&anyhow!(err.to_string())));

        let invalid = Scripted {
            replies: Mutex::new(vec![r#"{"a":1}"#.into(), r#"{"a":false}"#.into()]),
            seen: Mutex::new(vec![]),
        };
        let err = invalid
            .chat_json(&[ChatMessage::user("hi")], &schema, &ChatOptions::default())
            .unwrap_err();
        assert!(is_invalid_json_reply(&err), "{err:#}");
        assert!(err.to_string().contains("expected string"), "{err}");
        assert_eq!(invalid.seen.lock().len(), 2);
    }

    #[test]
    fn json_via_prompt_does_not_wrap_a_chat_failure() {
        struct Down;
        impl LlmProvider for Down {
            fn name(&self) -> String {
                "fake/down".into()
            }
            fn context_tokens(&self) -> usize {
                1024
            }
            fn chat(&self, _: &[ChatMessage], _: &ChatOptions) -> Result<String> {
                bail!("401 unauthorized")
            }
        }
        let schema = json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}});
        let err = Down
            .chat_json(&[ChatMessage::user("hi")], &schema, &ChatOptions::default())
            .unwrap_err();
        assert!(!is_invalid_json_reply(&err), "{err:#}");
        assert!(err.to_string().contains("401"), "{err}");
    }

    #[test]
    fn estimate_tokens_is_about_four_chars() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn block_on_works_inside_and_outside_a_runtime() {
        assert_eq!(block_on(async { 1 }), 1);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(rt.block_on(async { block_on(async { 2 }) }), 2);
    }
}
