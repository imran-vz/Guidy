use crate::settings::{PostProcessApiFormat, PostProcessProvider};
use log::{debug, error, info};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Serialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

/// A multi-message chat request for [`send_chat`] (the general entry point
/// used by `crate::llm`). Dictation post-processing keeps using
/// [`send_chat_completion`] / [`send_chat_completion_with_schema`].
#[derive(Debug, Clone, Default)]
pub struct ChatParams {
    /// `(role, content)` pairs: "system", "user" or "assistant". For the
    /// Anthropic format, system messages are sent as the top-level `system`.
    pub messages: Vec<(String, String)>,
    /// `(name, schema)`: request strict JSON-schema structured output. Only
    /// sent to OpenAI-format endpoints.
    pub json_schema: Option<(String, Value)>,
    /// Output budget. Required by Anthropic (default 4096); OpenAI-format
    /// endpoints get `max_tokens` only when set.
    pub max_tokens: Option<u32>,
    /// Sampling temperature, sent only when set.
    pub temperature: Option<f32>,
    /// Ask the endpoint to skip reasoning (see `reasoning_disable_params`).
    pub disable_reasoning: bool,
}

/// Why [`send_chat`] produced no reply. The Display text is the user-facing
/// message.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatError {
    /// The reply was cut off at the output token limit.
    Truncated(String),
    /// A successful response with an empty `choices` array.
    NoChoices,
    /// Anything else (HTTP, transport, parse errors).
    Failed(String),
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated(m) | Self::Failed(m) => f.write_str(m),
            Self::NoChoices => f.write_str("The response contained no choices"),
        }
    }
}

impl From<String> for ChatError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// The `String`-error API dictation post-processing uses, where a response
/// without choices counts as an empty reply.
fn into_string_result(result: Result<Option<String>, ChatError>) -> Result<Option<String>, String> {
    match result {
        Err(ChatError::NoChoices) => Ok(None),
        other => other.map_err(|e| e.to_string()),
    }
}

#[derive(Debug, Serialize)]
struct JsonSchema {
    name: String,
    strict: bool,
    schema: Value,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
    json_schema: JsonSchema,
}

#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude: Option<bool>,
}

/// Request fields used to ask an endpoint to skip reasoning/thinking.
/// Providers disagree on the field name and accepted values, so at most one of
/// these is set per request (see `reasoning_disable_params`).
#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
}

impl ReasoningParams {
    fn is_empty(&self) -> bool {
        self.reasoning_effort.is_none() && self.reasoning.is_none() && self.thinking.is_none()
    }
}

/// Pick the reasoning-disable request fields an endpoint understands.
/// Unknown endpoints get the common OpenAI-style field; if they reject it,
/// the request is retried without it (see `send_chat_completion_with_schema`).
fn reasoning_disable_params(provider: &PostProcessProvider) -> ReasoningParams {
    let base_url = provider.base_url.to_lowercase();
    if base_url.contains("api.deepseek.com") {
        // DeepSeek rejects reasoning_effort "none" and uses its own field:
        // https://api-docs.deepseek.com/guides/thinking_mode
        ReasoningParams {
            thinking: Some(serde_json::json!({ "type": "disabled" })),
            ..Default::default()
        }
    } else if provider.id == "openrouter" {
        // OpenRouter nested object; exclude:true also keeps reasoning text out
        // of the response so it can't pollute structured-output JSON parsing
        ReasoningParams {
            reasoning: Some(ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
            ..Default::default()
        }
    } else {
        ReasoningParams {
            reasoning_effort: Some("none".to_string()),
            ..Default::default()
        }
    }
}

/// What an endpoint (base_url|model) taught us by rejecting a request with a
/// 400/422 that then succeeded once adjusted. Applied up-front to later
/// requests so they skip the doomed attempts.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct LearnedParams {
    /// The endpoint rejects the reasoning-disable fields.
    drop_reasoning: bool,
    /// The endpoint rejects a non-default `temperature`.
    drop_temperature: bool,
    /// `Some(true)`: the output budget goes in `max_completion_tokens`;
    /// `Some(false)`: in `max_tokens` (overriding the model-name guess).
    use_max_completion_tokens: Option<bool>,
}

impl LearnedParams {
    /// What changed between the first request sent (`before`) and the one
    /// that succeeded (`after`).
    fn from_change(before: ParamShape, after: ParamShape) -> Self {
        let use_max_completion_tokens = if before.max_tokens && after.max_completion_tokens {
            Some(true)
        } else if before.max_completion_tokens && after.max_tokens {
            Some(false)
        } else {
            None
        };
        Self {
            drop_reasoning: before.reasoning && !after.reasoning,
            drop_temperature: before.temperature && !after.temperature,
            use_max_completion_tokens,
        }
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn merge(&mut self, other: Self) {
        self.drop_reasoning |= other.drop_reasoning;
        self.drop_temperature |= other.drop_temperature;
        if other.use_max_completion_tokens.is_some() {
            self.use_max_completion_tokens = other.use_max_completion_tokens;
        }
    }
}

/// Which optional fields a request carries (see [`LearnedParams::from_change`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ParamShape {
    max_tokens: bool,
    max_completion_tokens: bool,
    temperature: bool,
    reasoning: bool,
}

/// [`LearnedParams`] per endpoint (base_url|model), remembered for the
/// lifetime of the process.
fn learned_params_cache() -> &'static Mutex<HashMap<String, LearnedParams>> {
    static LEARNED: OnceLock<Mutex<HashMap<String, LearnedParams>>> = OnceLock::new();
    LEARNED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn endpoint_key(provider: &PostProcessProvider, model: &str) -> String {
    format!("{}|{}", provider.base_url.trim_end_matches('/'), model)
}

fn learned_params(key: &str) -> LearnedParams {
    learned_params_cache()
        .lock()
        .ok()
        .and_then(|map| map.get(key).copied())
        .unwrap_or_default()
}

fn remember_learned(key: String, learned: LearnedParams) {
    if learned.is_empty() {
        return;
    }
    if let Ok(mut map) = learned_params_cache().lock() {
        map.entry(key).or_default().merge(learned);
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// The output budget for OpenAI reasoning models, which reject
    /// `max_tokens` (see [`is_openai_reasoning_model`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(flatten)]
    reasoning: ReasoningParams,
}

/// OpenAI reasoning models (`gpt-5*`, `o1*`, `o3*`, `o4*`) reject
/// `max_tokens` (they take `max_completion_tokens`) and any non-default
/// `temperature`. Matched on the bare model name, as sent to OpenAI itself;
/// gateways that prefix it (`openai/gpt-5`) translate the fields themselves.
/// The `-chat` variants (`gpt-5-chat-latest`) are not reasoning models and
/// take both fields.
pub fn is_openai_reasoning_model(model: &str) -> bool {
    let name = model.trim().to_ascii_lowercase();
    ["gpt-5", "o1", "o3", "o4"]
        .iter()
        .any(|p| name.starts_with(p))
        && !name.contains("-chat")
}

/// Characters around a parameter name in an error message, incl. the
/// backslashes of a JSON-escaped one (`\"max_tokens\"`).
const NAME_QUOTES: [char; 6] = [':', ' ', '\'', '"', '`', '\\'];

/// The parameter name at the start of `rest` (after any quotes).
fn leading_param_name(rest: &str) -> Option<String> {
    let rest = rest.trim_start_matches(NAME_QUOTES);
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(rest.len());
    Some(rest[..end].to_string()).filter(|n| !n.is_empty())
}

/// The parameter a 400/422 error body names as the rejected one, if any.
/// `lower` is the lowercased error text.
fn rejected_param_name(lower: &str) -> Option<String> {
    // OpenAI: "Unsupported parameter: 'max_tokens' is not supported with
    // this model. Use 'max_completion_tokens' instead." /
    // "Unsupported value: 'temperature' does not support 0.2 ...".
    let named = ["unsupported parameter", "unsupported value"]
        .iter()
        .find_map(|marker| leading_param_name(&lower[lower.find(marker)? + marker.len()..]));
    if named.is_some() {
        return named;
    }
    // Structured fields, possibly JSON-escaped inside a string: OpenAI's
    // `"param": "temperature"` and pydantic's `"loc": ["body", "temperature"]`.
    let unescaped = lower.replace('\\', "");
    let field = |name: &str| {
        let marker = format!("\"{name}\"");
        let at = unescaped.find(&marker)? + marker.len();
        Some(unescaped[at..].trim_start_matches([' ', ':']))
    };
    if let Some(name) = field("param")
        .and_then(leading_param_name)
        .filter(|n| n != "null")
    {
        return Some(name);
    }
    let loc = field("loc")?.strip_prefix('[')?;
    let loc = &loc[..loc.find(']')?];
    loc.rsplit(',').next().and_then(leading_param_name)
}

/// Whether error text says `temperature` itself is the problem ("temperature
/// must be 1", "'temperature' is not supported", "Invalid value for
/// temperature"), as opposed to merely mentioning it (an echoed request body).
fn names_temperature_as_problem(lower: &str) -> bool {
    const AFTER: [&str; 6] = [
        "not supported",
        "unsupported",
        "does not support",
        "invalid",
        "must",
        "only",
    ];
    const BEFORE: [&str; 2] = ["unsupported", "invalid"];
    // Look only within the clause around each mention.
    let is_break = |c: char| matches!(c, '.' | ',' | ';' | '\n' | '{' | '}' | '[' | ']');
    lower.match_indices("temperature").any(|(at, word)| {
        let after = &lower[at + word.len()..];
        let after = &after[..after.find(is_break).unwrap_or(after.len())];
        let before = &lower[..at];
        let before = &before[before.rfind(is_break).map_or(0, |i| i + 1)..];
        AFTER.iter().any(|w| after.contains(w)) || BEFORE.iter().any(|w| before.contains(w))
    })
}

impl ChatCompletionRequest {
    /// Put the output budget and temperature in the fields `model` accepts.
    fn fit_params_to_model(&mut self) {
        if is_openai_reasoning_model(&self.model) {
            if let Some(n) = self.max_tokens.take() {
                self.max_completion_tokens = Some(n);
            }
            self.temperature = None;
        }
    }

    /// Apply what earlier requests to this endpoint learned (after
    /// [`Self::fit_params_to_model`], which it overrides).
    fn apply_learned(&mut self, learned: LearnedParams) {
        if learned.drop_reasoning {
            self.reasoning = ReasoningParams::default();
        }
        if learned.drop_temperature {
            self.temperature = None;
        }
        match learned.use_max_completion_tokens {
            Some(true) if self.max_tokens.is_some() => {
                self.max_completion_tokens = self.max_tokens.take();
            }
            Some(false) if self.max_completion_tokens.is_some() => {
                self.max_tokens = self.max_completion_tokens.take();
            }
            _ => {}
        }
    }

    fn shape(&self) -> ParamShape {
        ParamShape {
            max_tokens: self.max_tokens.is_some(),
            max_completion_tokens: self.max_completion_tokens.is_some(),
            temperature: self.temperature.is_some(),
            reasoning: !self.reasoning.is_empty(),
        }
    }

    /// After a 400/422 with `error_text`, drop or rename the parameter the
    /// endpoint rejected. Returns false when nothing could be changed (the
    /// error is then reported as is).
    fn adjust_after_rejection(&mut self, error_text: &str) -> bool {
        let lower = error_text.to_ascii_lowercase();
        let rejected = match rejected_param_name(&lower) {
            Some(n) => n,
            None if names_temperature_as_problem(&lower) && self.temperature.is_some() => {
                "temperature".to_string()
            }
            None if !self.reasoning.is_empty()
                && ["unsupported parameter", "reasoning", "thinking"]
                    .iter()
                    .any(|w| lower.contains(w)) =>
            {
                "reasoning".to_string()
            }
            None => return false,
        };
        match rejected.as_str() {
            "temperature" if self.temperature.is_some() => {
                self.temperature = None;
                true
            }
            "max_tokens" if self.max_tokens.is_some() => {
                self.max_completion_tokens = self.max_tokens.take();
                true
            }
            "max_completion_tokens" if self.max_completion_tokens.is_some() => {
                self.max_tokens = self.max_completion_tokens.take();
                true
            }
            r if r.starts_with("reasoning") || r.starts_with("thinking") => {
                let had = !self.reasoning.is_empty();
                self.reasoning = ReasoningParams::default();
                had
            }
            _ => false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
}

/// Output budget for Anthropic Messages requests, where `max_tokens` is
/// required. Ample for a cleaned-up dictation.
const ANTHROPIC_MAX_TOKENS: u32 = 4096;

#[derive(Debug, Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    /// Thinking and any other non-text blocks
    #[serde(other)]
    Other,
}

impl AnthropicMessagesResponse {
    fn into_text(self) -> Option<String> {
        let text: String = self
            .content
            .into_iter()
            .filter_map(|block| match block {
                AnthropicContentBlock::Text { text } => Some(text),
                AnthropicContentBlock::Other => None,
            })
            .collect();
        (!text.is_empty()).then_some(text)
    }
}

/// [`crate::llm::validate_base_url`] with this module's `String` errors.
fn check_base_url(provider: &PostProcessProvider) -> Result<(), String> {
    crate::llm::validate_base_url(provider).map_err(|e| e.to_string())
}

/// Build headers for API requests based on provider type.
///
/// Cloudflare AI Gateway: the provider key (the upstream's key, e.g. an
/// OpenAI key, or a Cloudflare API token for Workers AI models) goes in
/// `Authorization: Bearer`, like every OpenAI-format provider. Authenticated
/// gateways additionally want a `cf-aig-authorization` gateway token, which
/// Guidy does not send (it would need a second secret per provider): turn
/// gateway authentication off, or use the provider's direct endpoint.
fn build_headers(provider: &PostProcessProvider, api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    // Common headers
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(concat!("Guidy/", env!("CARGO_PKG_VERSION"))),
    );
    headers.insert("X-Title", HeaderValue::from_static("Guidy"));

    // Provider-specific auth headers
    if !api_key.is_empty() {
        if provider.api_format == PostProcessApiFormat::Anthropic {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(api_key)
                    .map_err(|e| format!("Invalid API key header value: {}", e))?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))
                    .map_err(|e| format!("Invalid authorization header value: {}", e))?,
            );
        }
    }

    Ok(headers)
}

/// Create an HTTP client with provider-specific headers
fn create_client(provider: &PostProcessProvider, api_key: &str) -> Result<reqwest::Client, String> {
    let headers = build_headers(provider, api_key)?;
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| report_reqwest_error("Failed to build HTTP client", &e))
}

/// Format a bounded error source chain.
///
/// `reqwest::Error`'s Display implementation intentionally gives only a short
/// summary. Nested causes contain the useful transport details, such as a
/// certificate validation failure, an HTTP/2 error, or a connection reset.
/// Callers must skip source types whose Display text can quote payload data.
fn error_source_chain(error: &(dyn StdError + 'static)) -> Vec<String> {
    let mut causes = Vec::new();
    let mut source = error.source();

    // Defensive cap in case a third-party error exposes a cyclic source chain.
    for _ in 0..16 {
        let Some(cause) = source else {
            break;
        };
        causes.push(cause.to_string());
        source = cause.source();
    }

    causes
}

fn reqwest_error_kinds(error: &reqwest::Error) -> String {
    let mut kinds = Vec::new();

    if error.is_builder() {
        kinds.push("builder");
    }
    if error.is_connect() {
        kinds.push("connect");
    }
    if error.is_request() {
        kinds.push("request");
    }
    if error.is_redirect() {
        kinds.push("redirect");
    }
    if error.is_timeout() {
        kinds.push("timeout");
    }
    if error.is_status() {
        kinds.push("status");
    }
    if error.is_body() {
        kinds.push("body");
    }
    if error.is_decode() {
        kinds.push("decode");
    }
    if error.is_upgrade() {
        kinds.push("upgrade");
    }

    if kinds.is_empty() {
        "unknown".to_string()
    } else {
        kinds.join(", ")
    }
}

fn sanitized_url(url: &reqwest::Url) -> String {
    let mut url = url.clone();

    // Custom endpoints should not contain credentials or query-string tokens,
    // but omit them from diagnostics in case one does.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);

    url.to_string()
}

fn sanitized_url_for_log(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|url| sanitized_url(&url))
        // Do not echo an invalid URL: the parse failure might have been caused
        // by sensitive data entered in the custom endpoint field.
        .unwrap_or_else(|_| "<invalid URL>".to_string())
}

fn report_reqwest_error(context: &str, error: &reqwest::Error) -> String {
    let kinds = reqwest_error_kinds(error);
    let url = error
        .url()
        .map(sanitized_url)
        .map(|url| format!(", url: {url}"))
        .unwrap_or_default();

    // serde_json's error text can quote values from a malformed response. That
    // response may contain transcription content, so retain the useful decode
    // classification but never put its nested source in logs or UI errors.
    let causes = if error.is_decode() {
        Vec::new()
    } else {
        error_source_chain(error)
    };
    let cause_details = if !causes.is_empty() {
        format!(": caused by: {}", causes.join(" -> "))
    } else if error.url().is_none() {
        // Reqwest's short Display text is safe when it cannot append a raw URL.
        format!(": {error}")
    } else {
        // The sanitized URL is already included above. Avoid formatting the
        // original error because its Display implementation includes the raw URL.
        String::new()
    };

    let details = format!("{context} (kind: {kinds}{url}){cause_details}");
    error!("{details}");
    details
}

/// Send a chat completion request to an OpenAI-compatible API
/// Returns Ok(Some(content)) on success, Ok(None) if response has no content,
/// or Err on actual errors (HTTP, parsing, etc.)
pub async fn send_chat_completion(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    disable_reasoning: bool,
) -> Result<Option<String>, String> {
    if provider.api_format == PostProcessApiFormat::Anthropic {
        return into_string_result(
            send_anthropic_message(
                provider,
                api_key,
                model,
                None,
                vec![ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                }],
                ANTHROPIC_MAX_TOKENS,
                None,
            )
            .await,
        );
    }

    send_chat_completion_with_schema(
        provider,
        api_key,
        model,
        prompt,
        None,
        None,
        disable_reasoning,
    )
    .await
}

/// Send a chat completion request with structured output support.
/// When json_schema is provided, uses structured outputs mode.
/// system_prompt is used as the system message when provided.
///
/// When disable_reasoning is set, the request carries the reasoning-disable
/// fields the endpoint is expected to understand. Not every OpenAI-compatible
/// endpoint accepts them (DeepSeek, Gemini's compat layer, and some OpenRouter
/// upstreams reject with 400), so a 400/422 answer to such a request triggers
/// one retry without the fields, and the rejection is remembered per
/// (base_url, model) so later requests skip the failing attempt entirely.
pub async fn send_chat_completion_with_schema(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    user_content: String,
    system_prompt: Option<String>,
    json_schema: Option<Value>,
    disable_reasoning: bool,
) -> Result<Option<String>, String> {
    // Build messages vector
    let mut messages = Vec::new();

    // Add system prompt if provided
    if let Some(system) = system_prompt {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }

    // Add user message
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

    into_string_result(
        send_openai_chat(
            provider,
            api_key,
            model,
            messages,
            json_schema.map(|schema| ("transcription_output".to_string(), schema)),
            None,
            None,
            disable_reasoning,
        )
        .await,
    )
}

/// Send a multi-message chat request in the provider's format (OpenAI-
/// compatible Chat Completions or Anthropic Messages). Returns the reply text,
/// `Ok(None)` when the reply's message has no content, and an error for HTTP
/// failures, truncated replies ([`ChatError::Truncated`]) and responses
/// without choices ([`ChatError::NoChoices`]).
pub async fn send_chat(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    params: ChatParams,
) -> Result<Option<String>, ChatError> {
    if provider.api_format == PostProcessApiFormat::Anthropic {
        let mut system = Vec::new();
        let mut messages = Vec::new();
        for (role, content) in params.messages {
            if role == "system" {
                system.push(content);
            } else {
                messages.push(ChatMessage { role, content });
            }
        }
        let system = (!system.is_empty()).then(|| system.join("\n\n"));
        return send_anthropic_message(
            provider,
            api_key,
            model,
            system,
            messages,
            params.max_tokens.unwrap_or(ANTHROPIC_MAX_TOKENS),
            params.temperature,
        )
        .await;
    }
    let messages = params
        .messages
        .into_iter()
        .map(|(role, content)| ChatMessage { role, content })
        .collect();
    send_openai_chat(
        provider,
        api_key,
        model,
        messages,
        params.json_schema,
        params.max_tokens,
        params.temperature,
        params.disable_reasoning,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn send_openai_chat(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    messages: Vec<ChatMessage>,
    json_schema: Option<(String, Value)>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    disable_reasoning: bool,
) -> Result<Option<String>, ChatError> {
    check_base_url(provider)?;
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base_url);

    debug!(
        "Sending chat completion request to: {}",
        sanitized_url_for_log(&url)
    );

    let client = create_client(provider, &api_key)?;

    // Build response_format if schema is provided
    let response_format = json_schema.map(|(name, schema)| ResponseFormat {
        format_type: "json_schema".to_string(),
        json_schema: JsonSchema {
            name,
            strict: true,
            schema,
        },
    });

    let key = endpoint_key(provider, model);
    let learned = learned_params(&key);
    let reasoning = if disable_reasoning {
        reasoning_disable_params(provider)
    } else {
        ReasoningParams::default()
    };

    let mut request_body = ChatCompletionRequest {
        model: model.to_string(),
        messages,
        stream: false,
        response_format,
        max_tokens,
        max_completion_tokens: None,
        temperature,
        reasoning,
    };
    request_body.fit_params_to_model();
    request_body.apply_learned(learned);
    let first_shape = request_body.shape();

    // A 400/422 is usually the endpoint rejecting one optional field: an
    // unsupported parameter (OpenAI reasoning models and `max_tokens` /
    // `temperature`) or the reasoning-disable fields (DeepSeek, Gemini's
    // compat layer, some OpenRouter upstreams). Each retry drops or renames
    // the rejected field, so an endpoint that rejects them one at a time
    // needs one retry per adjustable field (output budget, temperature,
    // reasoning), plus one spare.
    const MAX_RETRIES: usize = 4;
    let mut retries = 0;
    let response = loop {
        let response = client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| report_reqwest_error("HTTP request failed", &e))?;
        let status = response.status();
        debug!(
            "Chat completion response received with status {} over {:?} from {}",
            status,
            response.version(),
            sanitized_url(response.url())
        );
        if status.is_success() {
            break response;
        }
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read API error response", &e));
        let fail = || {
            Err(ChatError::Failed(format!(
                "API request failed with status {}: {}",
                status, error_text
            )))
        };
        if !matches!(status.as_u16(), 400 | 422) || retries >= MAX_RETRIES {
            return fail();
        }
        let had_reasoning = !request_body.reasoning.is_empty();
        if request_body.adjust_after_rejection(&error_text) {
            info!(
                "Endpoint rejected a request parameter (status {}): {}. Retrying without it",
                status, error_text
            );
        } else if had_reasoning {
            info!(
                "Endpoint rejected request with reasoning disabled (status {}): {}. Retrying without reasoning fields",
                status, error_text
            );
            request_body.reasoning = ReasoningParams::default();
        } else {
            return fail();
        }
        retries += 1;
    };

    let newly_learned = LearnedParams::from_change(first_shape, request_body.shape());
    if !newly_learned.is_empty() {
        info!(
            "Retry with adjusted parameters succeeded; '{}' (model '{}') will use them from now on: {:?}",
            sanitized_url_for_log(base_url),
            model,
            newly_learned
        );
        remember_learned(key, newly_learned);
    }

    let completion: ChatCompletionResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse API response", &e))?;

    let Some(choice) = completion.choices.into_iter().next() else {
        return Err(ChatError::NoChoices);
    };

    // A response cut off at the provider's output limit is incomplete; pasting
    // it would silently drop the end of the dictation (or, for structured
    // output, paste unparseable JSON).
    if choice.finish_reason.as_deref() == Some("length") {
        return Err(ChatError::Truncated(
            "Response was truncated at the provider's output token limit".to_string(),
        ));
    }

    Ok(choice.message.content)
}

/// Send a request to an Anthropic Messages API (`/messages`).
/// Used by every provider with the Anthropic format (the built-in Anthropic
/// provider and a custom provider set to it). None of them use structured
/// output, and thinking is off unless requested, so no reasoning fields are
/// needed.
async fn send_anthropic_message(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    system: Option<String>,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: Option<f32>,
) -> Result<Option<String>, ChatError> {
    check_base_url(provider)?;
    let url = format!("{}/messages", provider.base_url.trim_end_matches('/'));

    debug!(
        "Sending Anthropic Messages request to: {}",
        sanitized_url_for_log(&url)
    );

    let request_body = AnthropicMessagesRequest {
        model: model.to_string(),
        max_tokens,
        system,
        temperature,
        messages,
    };

    let response = create_client(provider, &api_key)?
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| report_reqwest_error("HTTP request failed", &e))?;
    let status = response.status();
    debug!(
        "Anthropic Messages response received with status {} over {:?} from {}",
        status,
        response.version(),
        sanitized_url(response.url())
    );

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read API error response", &e));
        return Err(ChatError::Failed(format!(
            "API request failed with status {}: {}",
            status, error_text
        )));
    }

    let message: AnthropicMessagesResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse API response", &e))?;

    // A response cut off at the token budget is incomplete; pasting it would
    // silently drop the end of the dictation.
    if message.stop_reason.as_deref() == Some("max_tokens") {
        return Err(ChatError::Truncated(format!(
            "Response was truncated at the {} token limit",
            max_tokens
        )));
    }

    Ok(message.into_text())
}

/// A model from a `/models` list, with the metadata some endpoints add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    /// Context window in tokens, when the list reports one (OpenRouter's
    /// `context_length`, the Vercel AI Gateway's `context_window`, ...).
    pub context_length: Option<usize>,
}

/// Fetch available models from an OpenAI-compatible API
/// Returns a list of model IDs
pub async fn fetch_models(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<Vec<String>, String> {
    Ok(fetch_models_with_info(provider, api_key)
        .await?
        .into_iter()
        .map(|m| m.id)
        .collect())
}

/// [`fetch_models`] plus per-model metadata.
pub async fn fetch_models_with_info(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<Vec<ModelInfo>, String> {
    check_base_url(provider)?;
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/models", base_url);

    debug!("Fetching models from: {}", sanitized_url_for_log(&url));

    let client = create_client(provider, &api_key)?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| report_reqwest_error("Failed to fetch models", &e))?;

    let status = response.status();
    debug!(
        "Model list response received with status {} over {:?} from {}",
        status,
        response.version(),
        sanitized_url(response.url())
    );
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read model list error", &e));
        return Err(format!(
            "Model list request failed ({}): {}",
            status, error_text
        ));
    }

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse model list response", &e))?;

    Ok(parse_model_list(&parsed))
}

/// Parse `{ data: [ { id, context_length? } ] }` or `[ "id", ... ]`.
pub fn parse_model_list(parsed: &Value) -> Vec<ModelInfo> {
    let mut models = Vec::new();
    // Handle OpenAI format: { data: [ { id: "..." }, ... ] }
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            let id = entry
                .get("id")
                .and_then(|i| i.as_str())
                .or_else(|| entry.get("name").and_then(|n| n.as_str()));
            if let Some(id) = id {
                models.push(ModelInfo {
                    id: id.to_string(),
                    context_length: context_length_of(entry),
                });
            }
        }
    }
    // Handle array format: [ "model1", "model2", ... ]
    else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(model) = entry.as_str() {
                models.push(ModelInfo {
                    id: model.to_string(),
                    context_length: None,
                });
            }
        }
    }
    models
}

/// The context window a model-list entry reports, under the field names the
/// gateways use. Values under 512 are treated as bogus.
fn context_length_of(entry: &Value) -> Option<usize> {
    const FIELDS: [&str; 4] = [
        "context_length",
        "context_window",
        "max_context_length",
        "max_input_tokens",
    ];
    let as_tokens = |v: &Value| {
        v.as_u64()
            .or_else(|| v.as_f64().filter(|f| *f > 0.0).map(|f| f as u64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    };
    FIELDS
        .iter()
        .find_map(|f| entry.get(*f).and_then(as_tokens))
        .or_else(|| {
            entry
                .get("top_provider")
                .and_then(|t| t.get("context_length"))
                .and_then(as_tokens)
        })
        .filter(|&n| n >= 512)
        .map(|n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Debug)]
    struct TestError {
        message: &'static str,
        source: Option<Box<TestError>>,
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.message)
        }
    }

    impl StdError for TestError {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &(dyn StdError + 'static))
        }
    }

    fn provider(id: &str, base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: id.to_string(),
            label: id.to_string(),
            base_url: base_url.to_string(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: PostProcessApiFormat::OpenAi,
        }
    }

    fn anthropic_format_provider(base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            api_format: PostProcessApiFormat::Anthropic,
            ..provider("custom", base_url)
        }
    }

    fn request_json(reasoning: ReasoningParams) -> Value {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            stream: false,
            response_format: None,
            max_tokens: None,
            max_completion_tokens: None,
            temperature: None,
            reasoning,
        };
        serde_json::to_value(&request).unwrap()
    }

    async fn serve_one_response(status: &str, body: &str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        format!("http://{address}")
    }

    #[test]
    fn error_source_chain_includes_all_nested_causes() {
        let error = TestError {
            message: "request failed",
            source: Some(Box::new(TestError {
                message: "TLS handshake failed",
                source: Some(Box::new(TestError {
                    message: "unknown certificate authority",
                    source: None,
                })),
            })),
        };

        assert_eq!(
            error_source_chain(&error),
            vec!["TLS handshake failed", "unknown certificate authority"]
        );
    }

    #[test]
    fn log_url_sanitization_removes_credentials_and_tokens() {
        let url = "https://user:password@example.com/v1/models?api_key=secret#private";
        assert_eq!(sanitized_url_for_log(url), "https://example.com/v1/models");
    }

    #[test]
    fn invalid_log_urls_are_not_echoed() {
        assert_eq!(
            sanitized_url_for_log("not a URL containing secret"),
            "<invalid URL>"
        );
    }

    #[tokio::test]
    async fn decode_error_does_not_echo_response_values() {
        let base_url =
            serve_one_response("200 OK", r#"{"choices":"PRIVATE TRANSCRIPTION CONTENT"}"#).await;
        let error = reqwest::get(base_url)
            .await
            .unwrap()
            .json::<ChatCompletionResponse>()
            .await
            .unwrap_err();

        let details = report_reqwest_error("Failed to parse API response", &error);
        assert!(details.contains("kind: decode"));
        assert!(!details.contains("PRIVATE TRANSCRIPTION CONTENT"));
    }

    #[tokio::test]
    async fn raw_error_url_is_not_reintroduced_without_a_source() {
        let base_url = serve_one_response("400 Bad Request", "bad request").await;
        let error = reqwest::get(format!(
            "{base_url}/private?api_key=SECRET_QUERY_TOKEN#private"
        ))
        .await
        .unwrap()
        .error_for_status()
        .unwrap_err();

        let details = report_reqwest_error("Request failed", &error);
        assert!(details.contains(&format!("url: {base_url}/private")));
        assert!(!details.contains("SECRET_QUERY_TOKEN"));
        assert!(!details.contains("#private"));
    }

    #[test]
    fn requests_explicitly_disable_streaming() {
        let json = request_json(ReasoningParams::default());
        assert_eq!(json["stream"], false);
    }

    #[test]
    fn default_reasoning_params_serialize_to_no_fields() {
        let json = request_json(ReasoningParams::default());
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn custom_provider_uses_top_level_reasoning_effort() {
        let params = reasoning_disable_params(&provider("custom", "http://localhost:11434/v1"));
        let json = request_json(params);
        assert_eq!(json["reasoning_effort"], "none");
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn openrouter_uses_nested_reasoning_object() {
        let params =
            reasoning_disable_params(&provider("openrouter", "https://openrouter.ai/api/v1"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert_eq!(json["reasoning"]["effort"], "none");
        assert_eq!(json["reasoning"]["exclude"], true);
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn deepseek_base_url_uses_thinking_disabled() {
        let params = reasoning_disable_params(&provider("custom", "https://api.deepseek.com"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert_eq!(json["thinking"]["type"], "disabled");
    }

    #[test]
    fn reasoning_params_is_empty_tracks_all_fields() {
        assert!(ReasoningParams::default().is_empty());
        assert!(!ReasoningParams {
            reasoning_effort: Some("none".to_string()),
            ..Default::default()
        }
        .is_empty());
        assert!(!ReasoningParams {
            thinking: Some(serde_json::json!({ "type": "disabled" })),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn learned_params_are_keyed_by_base_url_and_model() {
        let deepseek = provider("custom", "https://api.deepseek.com/");
        let key = endpoint_key(&deepseek, "deepseek-chat");
        assert_eq!(key, "https://api.deepseek.com|deepseek-chat");
        assert!(learned_params(&key).is_empty());
        remember_learned(
            key.clone(),
            LearnedParams {
                drop_reasoning: true,
                ..Default::default()
            },
        );
        remember_learned(
            key.clone(),
            LearnedParams {
                use_max_completion_tokens: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(
            learned_params(&key),
            LearnedParams {
                drop_reasoning: true,
                drop_temperature: false,
                use_max_completion_tokens: Some(true),
            }
        );
        // A different model on the same endpoint is tracked separately
        assert!(learned_params(&endpoint_key(&deepseek, "other-model")).is_empty());
    }

    #[test]
    fn learned_params_come_from_what_changed_and_apply_up_front() {
        let shape = |max_tokens, max_completion_tokens, temperature, reasoning| ParamShape {
            max_tokens,
            max_completion_tokens,
            temperature,
            reasoning,
        };
        let learned = LearnedParams::from_change(
            shape(true, false, true, true),
            shape(false, true, false, false),
        );
        assert_eq!(
            learned,
            LearnedParams {
                drop_reasoning: true,
                drop_temperature: true,
                use_max_completion_tokens: Some(true),
            }
        );
        // Nothing changed, nothing learned (a field the caller didn't send is
        // not a rejection).
        assert!(LearnedParams::from_change(
            shape(false, false, false, false),
            shape(false, false, false, false)
        )
        .is_empty());

        let mut r = ChatCompletionRequest {
            model: "my-gpt5".to_string(),
            messages: vec![],
            stream: false,
            response_format: None,
            max_tokens: Some(100),
            max_completion_tokens: None,
            temperature: Some(0.2),
            reasoning: ReasoningParams {
                reasoning_effort: Some("none".into()),
                ..Default::default()
            },
        };
        r.apply_learned(learned);
        assert_eq!((r.max_tokens, r.max_completion_tokens), (None, Some(100)));
        assert_eq!(r.temperature, None);
        assert!(r.reasoning.is_empty());

        // Learned `max_tokens` overrides the model-name guess.
        let mut r = ChatCompletionRequest {
            model: "o3".to_string(),
            max_tokens: Some(100),
            ..r
        };
        r.fit_params_to_model();
        r.apply_learned(LearnedParams {
            use_max_completion_tokens: Some(false),
            ..Default::default()
        });
        assert_eq!((r.max_tokens, r.max_completion_tokens), (Some(100), None));
    }

    #[test]
    fn placeholder_base_urls_are_refused_before_any_request() {
        let p = provider(
            "cloudflare_ai_gateway",
            "https://gateway.ai.cloudflare.com/v1/{account_id}/{gateway_id}/compat",
        );
        let err = check_base_url(&p).unwrap_err();
        assert!(err.contains("{account_id}"), "{err}");
        assert!(check_base_url(&provider("custom", "http://localhost:1/v1")).is_ok());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(send_chat_completion(
                &p,
                "k".into(),
                "m",
                "hi".into(),
                false,
            ))
            .unwrap_err();
        assert!(err.contains("{account_id}"), "{err}");
        let err = rt.block_on(fetch_models(&p, "k".into())).unwrap_err();
        assert!(err.contains("{account_id}"), "{err}");
    }

    #[test]
    fn model_lists_carry_context_lengths_when_reported() {
        let parsed = serde_json::json!({"data": [
            {"id": "openai/gpt-4o", "context_length": 128000},
            {"id": "anthropic/claude-sonnet-4", "context_window": "200000"},
            {"id": "meta/llama", "top_provider": {"context_length": 8192}},
            {"id": "bogus", "context_length": 0},
            {"name": "named-only"}
        ]});
        let models = parse_model_list(&parsed);
        let ctx: Vec<_> = models
            .iter()
            .map(|m| (m.id.as_str(), m.context_length))
            .collect();
        assert_eq!(
            ctx,
            vec![
                ("openai/gpt-4o", Some(128_000)),
                ("anthropic/claude-sonnet-4", Some(200_000)),
                ("meta/llama", Some(8_192)),
                ("bogus", None),
                ("named-only", None),
            ]
        );
        let plain = parse_model_list(&serde_json::json!(["a", "b"]));
        assert_eq!(plain[1].id, "b");
        assert_eq!(plain[1].context_length, None);
    }

    #[test]
    fn anthropic_format_uses_api_key_headers() {
        let headers = build_headers(
            &anthropic_format_provider("https://example.com/anthropic/v1"),
            "key",
        )
        .unwrap();
        assert_eq!(headers["x-api-key"], "key");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert!(headers.get(AUTHORIZATION).is_none());

        let headers =
            build_headers(&provider("custom", "http://localhost:11434/v1"), "key").unwrap();
        assert_eq!(headers[AUTHORIZATION], "Bearer key");
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn anthropic_request_carries_required_max_tokens() {
        let request = AnthropicMessagesRequest {
            model: "claude-haiku".to_string(),
            max_tokens: ANTHROPIC_MAX_TOKENS,
            system: None,
            temperature: None,
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["max_tokens"], ANTHROPIC_MAX_TOKENS);
        assert_eq!(json["messages"][0]["role"], "user");
        assert!(json.get("stream").is_none());
        assert!(json.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_response_keeps_only_text_blocks() {
        let response: AnthropicMessagesResponse = serde_json::from_str(
            r#"{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Hello"},{"type":"text","text":" world"}]}"#,
        )
        .unwrap();
        assert_eq!(response.into_text().as_deref(), Some("Hello world"));

        let empty: AnthropicMessagesResponse = serde_json::from_str(r#"{"content":[]}"#).unwrap();
        assert_eq!(empty.into_text(), None);
    }

    #[tokio::test]
    async fn anthropic_format_parses_messages_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Cleaned text"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku",
            "raw text".to_string(),
            true,
        )
        .await;
        assert_eq!(result, Ok(Some("Cleaned text".to_string())));
    }

    #[tokio::test]
    async fn chat_completion_rejects_truncated_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"choices":[{"message":{"content":"Cut off"},"finish_reason":"length"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &provider("custom", &base_url),
            "key".to_string(),
            "test-model",
            "raw text".to_string(),
            false,
        )
        .await;
        assert!(result.unwrap_err().contains("truncated"));
    }

    /// Serve one response and hand back the raw request (headers + body).
    async fn serve_and_capture(
        body: &'static str,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let (url, mut requests) = serve_sequence(vec![("200 OK", body)]).await;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            if let Some(r) = requests.recv().await {
                let _ = tx.send(r);
            }
        });
        (url, rx)
    }

    /// Answer one request per connection with each `(status, body)` in turn,
    /// sending every raw request received on the channel.
    async fn serve_sequence(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buf = [0_u8; 4096];
                // Read until the whole body (per Content-Length) has arrived.
                loop {
                    let n = stream.read(&mut buf).await.unwrap();
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(split) = text.find("\r\n\r\n") {
                        let len = text[..split]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= split + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                let _ = tx.send(String::from_utf8_lossy(&request).to_string());
            }
        });
        (format!("http://{address}"), rx)
    }

    fn request_body(raw: &str) -> Value {
        let body = &raw[raw.find("\r\n\r\n").unwrap() + 4..];
        serde_json::from_str(body).unwrap()
    }

    #[tokio::test]
    async fn send_chat_openai_sends_messages_schema_and_limits() {
        let (base_url, rx) = serve_and_capture(
            r#"{"choices":[{"message":{"content":"{\"a\":1}"},"finish_reason":"stop"}]}"#,
        )
        .await;
        let result = send_chat(
            &provider("custom", &base_url),
            "key".to_string(),
            "m",
            ChatParams {
                messages: vec![
                    ("system".into(), "sys".into()),
                    ("user".into(), "hello".into()),
                ],
                json_schema: Some((
                    "meeting_summary".into(),
                    serde_json::json!({"type":"object"}),
                )),
                max_tokens: Some(100),
                temperature: Some(0.2),
                disable_reasoning: false,
            },
        )
        .await;
        assert_eq!(result, Ok(Some("{\"a\":1}".to_string())));
        let body = request_body(&rx.await.unwrap());
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hello");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "meeting_summary"
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["max_tokens"], 100);
        assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
    }

    #[tokio::test]
    async fn send_chat_anthropic_moves_system_to_top_level() {
        let (base_url, rx) =
            serve_and_capture(r#"{"type":"message","content":[{"type":"text","text":"ok"}]}"#)
                .await;
        let result = send_chat(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude",
            ChatParams {
                messages: vec![
                    ("system".into(), "be brief".into()),
                    ("user".into(), "hi".into()),
                ],
                max_tokens: Some(8000),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(result, Ok(Some("ok".to_string())));
        let body = request_body(&rx.await.unwrap());
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["max_tokens"], 8000);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body.get("temperature").is_none());
    }

    #[tokio::test]
    async fn anthropic_format_rejects_truncated_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"type":"message","stop_reason":"max_tokens","content":[{"type":"text","text":"Cut off"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku",
            "raw text".to_string(),
            true,
        )
        .await;
        assert!(result.unwrap_err().contains("truncated"));
    }
    #[test]
    fn reasoning_models_get_max_completion_tokens_and_no_temperature() {
        for model in ["gpt-5", "gpt-5-mini", "o1-mini", "o3", "O4-mini"] {
            assert!(is_openai_reasoning_model(model), "{model}");
        }
        for model in [
            "gpt-4o",
            "gpt-4.1-mini",
            "openai/gpt-5",
            "llama-3.3-70b",
            "gpt-5-chat-latest",
            "gpt-5.1-chat-latest",
        ] {
            assert!(!is_openai_reasoning_model(model), "{model}");
        }
        let body = |model: &str| {
            let mut request = ChatCompletionRequest {
                model: model.to_string(),
                messages: vec![],
                stream: false,
                response_format: None,
                max_tokens: Some(100),
                max_completion_tokens: None,
                temperature: Some(0.2),
                reasoning: ReasoningParams::default(),
            };
            request.fit_params_to_model();
            serde_json::to_value(&request).unwrap()
        };
        let json = body("gpt-5-mini");
        assert_eq!(json["max_completion_tokens"], 100);
        assert!(json.get("max_tokens").is_none());
        assert!(json.get("temperature").is_none());
        let json = body("gpt-4o");
        assert_eq!(json["max_tokens"], 100);
        assert!(json.get("max_completion_tokens").is_none());
        assert!(json.get("temperature").is_some());
    }

    #[test]
    fn rejected_parameters_are_dropped_or_renamed() {
        let request = || ChatCompletionRequest {
            model: "m".to_string(),
            messages: vec![],
            stream: false,
            response_format: None,
            max_tokens: Some(100),
            max_completion_tokens: None,
            temperature: Some(0.2),
            reasoning: ReasoningParams {
                reasoning_effort: Some("none".into()),
                ..Default::default()
            },
        };
        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"error":{"message":"Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.","param":"max_tokens"}}"#
        ));
        assert_eq!((r.max_tokens, r.max_completion_tokens), (None, Some(100)));
        assert_eq!(r.temperature, Some(0.2));
        assert!(!r.reasoning.is_empty());

        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"error":{"message":"Unsupported value: 'temperature' does not support 0.2 with this model. Only the default (1) value is supported."}}"#
        ));
        assert_eq!(r.temperature, None);
        assert_eq!(r.max_tokens, Some(100));

        let mut r = request();
        assert!(r.adjust_after_rejection("temperature must be 1 for this model"));
        assert_eq!(r.temperature, None);

        let mut r = request();
        assert!(r.adjust_after_rejection("Unsupported parameter: 'reasoning_effort'"));
        assert!(r.reasoning.is_empty());
        assert_eq!(r.temperature, Some(0.2));

        // A JSON-escaped name inside an error string.
        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"error":"{\"message\":\"Unsupported parameter: \\\"max_tokens\\\" is not supported\"}"}"#
        ));
        assert_eq!((r.max_tokens, r.max_completion_tokens), (None, Some(100)));
        let mut r = request();
        assert!(r.adjust_after_rejection(r#"Unsupported parameter: \"max_tokens\""#));
        assert_eq!(r.max_completion_tokens, Some(100));

        // OpenAI's `param` field and pydantic's `loc` name the field.
        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"error":{"message":"Invalid request","param":"temperature"}}"#
        ));
        assert_eq!(r.temperature, None);
        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"detail":[{"loc":["body","temperature"],"msg":"Input should be 1"}]}"#
        ));
        assert_eq!(r.temperature, None);
        assert!(!r.reasoning.is_empty());

        // A pydantic 422 rejecting reasoning_effort while echoing the request
        // body (temperature included) keeps the temperature.
        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"detail":[{"type":"extra_forbidden","loc":["body","reasoning_effort"],"msg":"Extra inputs are not permitted","input":"none"}],"body":{"temperature":0.2,"max_tokens":100,"reasoning_effort":"none"}}"#
        ));
        assert!(r.reasoning.is_empty());
        assert_eq!(r.temperature, Some(0.2));
        // Same without a `loc`: temperature is only mentioned, not blamed.
        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"error":"Extra inputs are not permitted","body":{"temperature":0.2,"reasoning_effort":"none"}}"#
        ));
        assert!(r.reasoning.is_empty());
        assert_eq!(r.temperature, Some(0.2));
        // `"param": null` names nothing.
        let mut r = request();
        assert!(r.adjust_after_rejection(
            r#"{"error":{"message":"temperature is not supported for this model","param":null}}"#
        ));
        assert_eq!(r.temperature, None);
        let mut r = request();
        assert!(r.adjust_after_rejection("Invalid value for temperature: 0.2"));
        assert_eq!(r.temperature, None);

        // Nothing identifiable: left to the caller.
        let mut r = request();
        assert!(!r.adjust_after_rejection("model not found"));
        assert!(!r.adjust_after_rejection(r#"{"error":"bad request","temperature":0.2}"#));
        r.temperature = None;
        assert!(!r.adjust_after_rejection("Unsupported value: 'temperature'"));
    }

    #[tokio::test]
    async fn a_rejected_parameter_is_retried_without_it() {
        let (base_url, mut requests) = serve_sequence(vec![
            (
                "400 Bad Request",
                r#"{"error":{"message":"Unsupported value: 'temperature' does not support 0.2 with this model."}}"#,
            ),
            (
                "200 OK",
                r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#,
            ),
        ])
        .await;
        let result = send_chat(
            &provider("custom", &base_url),
            "key".to_string(),
            "some-reasoning-model",
            ChatParams {
                messages: vec![("user".into(), "hi".into())],
                max_tokens: Some(50),
                temperature: Some(0.2),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(result, Ok(Some("ok".to_string())));
        let first = request_body(&requests.recv().await.unwrap());
        assert!(first.get("temperature").is_some());
        let second = request_body(&requests.recv().await.unwrap());
        assert!(second.get("temperature").is_none());
        assert_eq!(second["max_tokens"], 50);
    }

    #[tokio::test]
    async fn rejections_one_field_at_a_time_are_all_retried_and_remembered() {
        const OK: &str = r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#;
        let (base_url, mut requests) = serve_sequence(vec![
            (
                "400 Bad Request",
                r#"{"error":{"message":"Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead."}}"#,
            ),
            (
                "400 Bad Request",
                r#"{"error":{"message":"Unsupported value: 'temperature' does not support 0.2 with this model."}}"#,
            ),
            (
                "400 Bad Request",
                r#"{"error":{"message":"Unsupported parameter: 'reasoning_effort'"}}"#,
            ),
            ("200 OK", OK),
            ("200 OK", OK),
        ])
        .await;
        // An undetected OpenAI reasoning model (Azure deployment name).
        let params = || ChatParams {
            messages: vec![("user".into(), "hi".into())],
            max_tokens: Some(50),
            temperature: Some(0.2),
            disable_reasoning: true,
            ..Default::default()
        };
        let p = provider("custom", &base_url);
        let result = send_chat(&p, "key".to_string(), "my-gpt5", params()).await;
        assert_eq!(result, Ok(Some("ok".to_string())));
        for _ in 0..3 {
            requests.recv().await.unwrap();
        }
        let fourth = request_body(&requests.recv().await.unwrap());
        assert!(fourth.get("max_tokens").is_none());
        assert_eq!(fourth["max_completion_tokens"], 50);
        assert!(fourth.get("temperature").is_none());
        assert!(fourth.get("reasoning_effort").is_none());

        // The next request goes straight to the adjusted form.
        let result = send_chat(&p, "key".to_string(), "my-gpt5", params()).await;
        assert_eq!(result, Ok(Some("ok".to_string())));
        assert_eq!(request_body(&requests.recv().await.unwrap()), fourth);
    }

    #[tokio::test]
    async fn chat_without_choices_is_distinguished_from_an_empty_message() {
        let (base_url, _requests) = serve_sequence(vec![
            ("200 OK", r#"{"choices":[]}"#),
            (
                "200 OK",
                r#"{"choices":[{"message":{"content":null},"finish_reason":"stop"}]}"#,
            ),
            ("200 OK", r#"{"choices":[]}"#),
        ])
        .await;
        let p = provider("custom", &base_url);
        let params = || ChatParams {
            messages: vec![("user".into(), "hi".into())],
            ..Default::default()
        };
        assert_eq!(
            send_chat(&p, "key".to_string(), "m", params()).await,
            Err(ChatError::NoChoices)
        );
        assert_eq!(
            send_chat(&p, "key".to_string(), "m", params()).await,
            Ok(None)
        );
        // Dictation post-processing still treats it as an empty reply.
        assert_eq!(
            send_chat_completion(&p, "key".to_string(), "m", "hi".into(), false).await,
            Ok(None)
        );
    }

    #[tokio::test]
    async fn an_unidentified_rejection_is_reported_without_retrying() {
        let (base_url, mut requests) =
            serve_sequence(vec![("400 Bad Request", "invalid model")]).await;
        let result = send_chat(
            &provider("custom", &base_url),
            "key".to_string(),
            "m",
            ChatParams {
                messages: vec![("user".into(), "hi".into())],
                temperature: Some(0.2),
                ..Default::default()
            },
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("invalid model"));
        assert!(requests.recv().await.is_some());
        assert!(requests.recv().await.is_none());
    }
}
