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
    match feature {
        Feature::PostProcess => {
            let id = settings.post_process_provider_id.clone();
            let model = settings
                .post_process_models
                .get(&id)
                .cloned()
                .unwrap_or_default();
            (id, model)
        }
        Feature::Meetings => {
            let id = settings.meetings.llm_provider_id.clone();
            let model = settings
                .meetings
                .llm_models
                .get(&id)
                .filter(|m| !m.trim().is_empty())
                .or_else(|| settings.post_process_models.get(&id))
                .cloned()
                .unwrap_or_default();
            (id, model)
        }
    }
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
    if provider_id == BUILTIN_LLM_PROVIDER_ID {
        return Ok(Arc::new(builtin::BuiltinProvider::new(app, model)?));
    }
    if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        return Ok(Arc::new(apple::AppleProvider::new()?));
    }
    let provider = settings
        .post_process_provider(provider_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown LLM provider '{provider_id}'"))?;
    validate_base_url(&provider)?;
    if model.trim().is_empty() {
        bail!("no model selected for {}", provider.label);
    }
    let api_key = crate::settings::provider_api_key(settings, provider_id);
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
/// ship with `{placeholders}`; refuse to call them until filled in.
pub fn validate_base_url(provider: &PostProcessProvider) -> Result<()> {
    let url = provider.base_url.trim();
    if url.is_empty() {
        bail!("{} has no base URL", provider.label);
    }
    if let (Some(a), Some(b)) = (url.find('{'), url.find('}')) {
        if a < b {
            bail!(
                "Set the {} part of the {} base URL",
                &url[a..=b],
                provider.label
            );
        }
    }
    Ok(())
}

/// Check that a provider is usable. Returns a short human-readable status
/// for the "Test connection" button; the error text explains what's wrong.
/// Blocking. Never logs or returns the key.
pub fn test_connection(app: &AppHandle, provider_id: &str) -> Result<String> {
    let settings = crate::settings::get_settings(app);
    if provider_id == BUILTIN_LLM_PROVIDER_ID {
        let model = settings
            .meetings
            .llm_models
            .get(provider_id)
            .cloned()
            .unwrap_or_default();
        let provider = builtin::BuiltinProvider::new(app, &model)?;
        builtin::BuiltinLlm::global().ping()?;
        return Ok(format!("Built-in model ready: {}", provider.model_label()));
    }
    if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        apple::AppleProvider::new()?;
        return Ok("Apple Intelligence is available".to_string());
    }
    let provider = settings
        .post_process_provider(provider_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown LLM provider '{provider_id}'"))?;
    validate_base_url(&provider)?;
    let api_key = crate::settings::provider_api_key(&settings, provider_id);
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
            let model = settings
                .meetings
                .llm_models
                .get(provider_id)
                .filter(|m| !m.trim().is_empty())
                .or_else(|| settings.post_process_models.get(provider_id))
                .cloned()
                .unwrap_or_default();
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
            http.chat(
                &[ChatMessage::user("Reply with OK.")],
                &ChatOptions {
                    max_tokens: Some(1),
                    ..Default::default()
                },
            )
            .map(|_| format!("Connected to {} ({model} replied)", provider.label))
        }
    }
}

/// Model ids a provider offers (the built-in runtime: downloaded GGUFs).
/// Blocking.
pub fn list_models(app: &AppHandle, provider_id: &str) -> Result<Vec<String>> {
    if provider_id == BUILTIN_LLM_PROVIDER_ID {
        return Ok(local_models::LocalModels::new(app)?
            .list()
            .into_iter()
            .filter(|m| m.downloaded)
            .map(|m| m.id)
            .collect());
    }
    if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        return Ok(vec![
            crate::settings::APPLE_INTELLIGENCE_DEFAULT_MODEL_ID.to_string()
        ]);
    }
    let settings = crate::settings::get_settings(app);
    let provider = settings
        .post_process_provider(provider_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown LLM provider '{provider_id}'"))?;
    validate_base_url(&provider)?;
    let api_key = crate::settings::provider_api_key(&settings, provider_id);
    block_on(http::list_models(&provider, api_key))
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
    Err(last_err.unwrap_or_else(|| anyhow!("no valid JSON reply")))
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
