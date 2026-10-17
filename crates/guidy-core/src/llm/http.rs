//! [`HttpProvider`]: any `post_process_providers` entry (OpenAI-compatible or
//! Anthropic format) as an [`LlmProvider`], through [`crate::llm_client`].

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{
    block_on, extract_json, json_via_prompt, strip_think, validate_json, ChatMessage, ChatOptions,
    LlmProvider, CANCELLED,
};
use crate::llm_client::{self, ChatParams};
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
            disable_reasoning: matches!(self.provider.id.as_str(), "custom" | "openrouter"),
        }
    }

    fn send(&self, params: ChatParams, opts: &ChatOptions) -> Result<String> {
        let provider = self.provider.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let cancel = opts.cancel.clone();
        let reply = block_on(async move {
            let request = llm_client::send_chat(&provider, api_key, &model, params);
            tokio::pin!(request);
            loop {
                if cancel
                    .as_ref()
                    .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
                {
                    return Err(CANCELLED.to_string());
                }
                if let Ok(r) = tokio::time::timeout(Duration::from_millis(200), &mut request).await
                {
                    return r;
                }
            }
        });
        match reply {
            Ok(Some(text)) => Ok(strip_think(&text).to_string()),
            Ok(None) => Err(anyhow!("{} returned an empty reply", self.provider.label)),
            Err(e) if e == CANCELLED => Err(anyhow!(CANCELLED)),
            Err(e) => Err(anyhow!("{}: {e}", self.provider.label)),
        }
    }
}

/// Context window assumed per provider. Cloud models are large enough that
/// meeting transcripts are summarized in one pass; Custom (often a local
/// Ollama with a small default context) is chunked.
pub fn context_tokens_for(provider: &PostProcessProvider) -> usize {
    match provider.id.as_str() {
        "anthropic" => 200_000,
        "custom" => 8_192,
        "groq" | "cerebras" => 32_000,
        CLOUDFLARE_WORKERS_AI_PROVIDER_ID => 24_000,
        _ => 128_000,
    }
}

impl LlmProvider for HttpProvider {
    fn name(&self) -> String {
        format!("{}/{}", self.provider.id, self.model)
    }

    fn context_tokens(&self) -> usize {
        context_tokens_for(&self.provider)
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
        if self.provider.supports_structured_output
            && self.provider.api_format == PostProcessApiFormat::OpenAi
        {
            let mut params = self.params(messages, opts);
            params.json_schema = Some(("response".to_string(), json_schema.clone()));
            match self
                .send(params, opts)
                .and_then(|reply| extract_json(&reply))
                .and_then(|v| validate_json(&v, json_schema).map(|_| v))
            {
                Ok(v) => return Ok(v),
                Err(e) if super::is_cancelled_error(&e) => return Err(e),
                Err(e) => log::warn!(
                    "{}: structured output failed ({e}); falling back to prompt-only JSON",
                    self.name()
                ),
            }
        }
        json_via_prompt(self, messages, json_schema, opts)
    }
}

/// List a provider's models. Cloudflare Workers AI has no OpenAI-style
/// `/models`, so it uses its model search API.
pub async fn list_models(provider: &PostProcessProvider, api_key: String) -> Result<Vec<String>> {
    if provider.id == CLOUDFLARE_WORKERS_AI_PROVIDER_ID {
        return cloudflare_models(provider, &api_key).await;
    }
    llm_client::fetch_models(provider, api_key)
        .await
        .map_err(|e| anyhow!(e))
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
    Ok(parse_cloudflare_models(&json))
}

pub fn parse_cloudflare_models(json: &Value) -> Vec<String> {
    json.get("result")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|m| m.get("name").and_then(Value::as_str))
                .map(str::to_string)
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
