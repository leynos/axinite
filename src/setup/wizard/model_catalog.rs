//! Model listing helpers for provider APIs (Anthropic, OpenAI, Ollama,
//! OpenAI-compatible), with static fallbacks.

use super::*;

pub(super) struct OpenAICompatModelsRequest<'a> {
    pub(super) base_url: &'a str,
    pub(super) cached_key: Option<&'a str>,
}

/// Fetch models from the Anthropic API.
///
/// Returns `(model_id, display_label)` pairs. Falls back to static defaults on error.
pub(super) enum AnthropicAuth {
    ApiKey(String),
    OAuth(String),
}

pub(super) fn resolve_anthropic_auth(cached_credential: Option<&str>) -> Option<AnthropicAuth> {
    if let Some(credential) = cached_credential.filter(|credential| {
        !credential.is_empty() && *credential != crate::config::OAUTH_PLACEHOLDER
    }) {
        return if credential.starts_with("sk-ant-oat") {
            Some(AnthropicAuth::OAuth(credential.to_string()))
        } else {
            Some(AnthropicAuth::ApiKey(credential.to_string()))
        };
    }

    let api_key =
        crate::config::helpers::optional_env(crate::config::helpers::EnvKey("ANTHROPIC_API_KEY"))
            .ok()
            .flatten()
            .filter(|key| !key.is_empty() && key != crate::config::OAUTH_PLACEHOLDER);
    if let Some(api_key) = api_key {
        return Some(AnthropicAuth::ApiKey(api_key));
    }

    crate::config::helpers::optional_env(crate::config::helpers::EnvKey("ANTHROPIC_OAUTH_TOKEN"))
        .ok()
        .flatten()
        .filter(|token| !token.is_empty())
        .map(AnthropicAuth::OAuth)
}

fn anthropic_request(client: &reqwest::Client, auth: &AnthropicAuth) -> reqwest::RequestBuilder {
    let request = client
        .get("https://api.anthropic.com/v1/models")
        .header("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(5));

    match auth {
        AnthropicAuth::ApiKey(key) => request.header("x-api-key", key),
        AnthropicAuth::OAuth(token) => request
            .bearer_auth(token)
            .header("anthropic-beta", "oauth-2025-04-20"),
    }
}

async fn parse_anthropic_models_response(
    resp: reqwest::Response,
) -> Result<Vec<(String, String)>, reqwest::Error> {
    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: String,
    }

    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        data: Vec<ModelEntry>,
    }

    let body = resp.json::<ModelsResponse>().await?;
    let mut models: Vec<(String, String)> = body
        .data
        .into_iter()
        .filter(|model| !model.id.contains("embedding") && !model.id.contains("audio"))
        .map(|model| {
            let label = model.id.clone();
            (model.id, label)
        })
        .collect();
    models.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(models)
}

fn anthropic_static_defaults() -> Vec<(String, String)> {
    vec![
        (
            "claude-opus-4-6".into(),
            "Claude Opus 4.6 (latest flagship)".into(),
        ),
        ("claude-sonnet-4-6".into(), "Claude Sonnet 4.6".into()),
        ("claude-opus-4-5".into(), "Claude Opus 4.5".into()),
        ("claude-sonnet-4-5".into(), "Claude Sonnet 4.5".into()),
        ("claude-haiku-4-5".into(), "Claude Haiku 4.5 (fast)".into()),
    ]
}

pub(super) async fn fetch_anthropic_models(cached_key: Option<&str>) -> Vec<(String, String)> {
    let defaults = anthropic_static_defaults();
    let Some(auth) = resolve_anthropic_auth(cached_key) else {
        return defaults;
    };
    let client = reqwest::Client::new();
    let req = anthropic_request(&client, &auth);
    let resp = match req.send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return defaults,
    };
    match parse_anthropic_models_response(resp).await {
        Ok(list) if !list.is_empty() => list,
        _ => defaults,
    }
}

/// Fetch models from the OpenAI API.
///
/// Returns `(model_id, display_label)` pairs. Falls back to static defaults on error.
pub(super) async fn fetch_openai_models(cached_key: Option<&str>) -> Vec<(String, String)> {
    let static_defaults = vec![
        (
            "gpt-5.3-codex".into(),
            "GPT-5.3 Codex (latest flagship)".into(),
        ),
        ("gpt-5.2-codex".into(), "GPT-5.2 Codex".into()),
        ("gpt-5.2".into(), "GPT-5.2".into()),
        (
            "gpt-5.1-codex-mini".into(),
            "GPT-5.1 Codex Mini (fast)".into(),
        ),
        ("gpt-5".into(), "GPT-5".into()),
        ("gpt-5-mini".into(), "GPT-5 Mini".into()),
        ("gpt-4.1".into(), "GPT-4.1".into()),
        ("gpt-4.1-mini".into(), "GPT-4.1 Mini".into()),
        ("o4-mini".into(), "o4-mini (fast reasoning)".into()),
        ("o3".into(), "o3 (reasoning)".into()),
    ];

    let api_key = cached_key
        .map(String::from)
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .filter(|k| !k.is_empty());

    let api_key = match api_key {
        Some(k) => k,
        None => return static_defaults,
    };

    let client = reqwest::Client::new();
    let resp = match client
        .get("https://api.openai.com/v1/models")
        .bearer_auth(&api_key)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return static_defaults,
    };

    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: String,
    }
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        data: Vec<ModelEntry>,
    }

    match resp.json::<ModelsResponse>().await {
        Ok(body) => {
            let mut models: Vec<(String, String)> = body
                .data
                .into_iter()
                .filter(|m| is_openai_chat_model(&m.id))
                .map(|m| {
                    let label = m.id.clone();
                    (m.id, label)
                })
                .collect();
            if models.is_empty() {
                return static_defaults;
            }
            sort_openai_models(&mut models);
            models
        }
        Err(_) => static_defaults,
    }
}

/// Prefixes identifying OpenAI chat-capable model families.
const CHAT_FAMILY_PREFIXES: &[&str] = &["gpt-", "chatgpt-", "o1", "o3", "o4", "o5"];

/// Substrings identifying non-chat model variants (audio, embeddings, etc.).
const NON_CHAT_MARKERS: &[&str] = &[
    "realtime",
    "audio",
    "transcribe",
    "tts",
    "embedding",
    "moderation",
    "image",
];

/// Report whether a lowercased model id belongs to a chat-capable family.
fn is_chat_family(id: &str) -> bool {
    CHAT_FAMILY_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
}

/// Report whether a lowercased model id names a non-chat variant.
fn is_non_chat_variant(id: &str) -> bool {
    NON_CHAT_MARKERS.iter().any(|marker| id.contains(marker))
}

pub(super) fn is_openai_chat_model(model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();
    is_chat_family(&id) && !is_non_chat_variant(&id)
}

fn openai_model_priority(model_id: &str) -> usize {
    let id = model_id.to_ascii_lowercase();

    const EXACT_PRIORITY: &[&str] = &[
        "gpt-5.3-codex",
        "gpt-5.2-codex",
        "gpt-5.2",
        "gpt-5.1-codex-mini",
        "gpt-5",
        "gpt-5-mini",
        "gpt-5-nano",
        "o4-mini",
        "o3",
        "o1",
        "gpt-4.1",
        "gpt-4.1-mini",
        "gpt-4o",
        "gpt-4o-mini",
    ];
    if let Some(pos) = EXACT_PRIORITY.iter().position(|m| id == *m) {
        return pos;
    }

    const PREFIX_PRIORITY: &[&str] = &[
        "gpt-5.", "gpt-5-", "o3-", "o4-", "o1-", "gpt-4.1-", "gpt-4o-", "gpt-3.5-", "chatgpt-",
    ];
    if let Some(pos) = PREFIX_PRIORITY
        .iter()
        .position(|prefix| id.starts_with(prefix))
    {
        return EXACT_PRIORITY.len() + pos;
    }

    EXACT_PRIORITY.len() + PREFIX_PRIORITY.len() + 1
}

pub(super) fn sort_openai_models(models: &mut [(String, String)]) {
    models.sort_by(|a, b| {
        openai_model_priority(&a.0)
            .cmp(&openai_model_priority(&b.0))
            .then_with(|| a.0.cmp(&b.0))
    });
}

/// Fetch installed models from a local Ollama instance.
///
/// Returns `(model_name, display_label)` pairs. Falls back to static defaults on error.
pub(super) async fn fetch_ollama_models(base_url: &str) -> Vec<(String, String)> {
    let static_defaults = vec![
        ("llama3".into(), "llama3".into()),
        ("mistral".into(), "mistral".into()),
        ("codellama".into(), "codellama".into()),
    ];

    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let resp = match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(_) => return static_defaults,
        Err(_) => {
            print_info("Could not connect to Ollama. Is it running?");
            return static_defaults;
        }
    };

    #[derive(serde::Deserialize)]
    struct ModelEntry {
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct TagsResponse {
        models: Vec<ModelEntry>,
    }

    match resp.json::<TagsResponse>().await {
        Ok(body) => {
            let models: Vec<(String, String)> = body
                .models
                .into_iter()
                .map(|m| {
                    let label = m.name.clone();
                    (m.name, label)
                })
                .collect();
            if models.is_empty() {
                return static_defaults;
            }
            models
        }
        Err(_) => static_defaults,
    }
}

/// Fetch models from a generic OpenAI-compatible /v1/models endpoint.
///
/// Used for registry providers like Groq, NVIDIA NIM, etc.
pub(super) async fn fetch_openai_compatible_models(
    req: OpenAICompatModelsRequest<'_>,
) -> Vec<(String, String)> {
    let OpenAICompatModelsRequest {
        base_url,
        cached_key,
    } = req;

    if base_url.is_empty() {
        return vec![];
    }

    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req = client.get(&url).timeout(std::time::Duration::from_secs(5));
    if let Some(key) = cached_key {
        req = req.bearer_auth(key);
    }

    let resp = match req.send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return vec![],
    };

    #[derive(serde::Deserialize)]
    struct Model {
        id: String,
    }
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        data: Vec<Model>,
    }

    match resp.json::<ModelsResponse>().await {
        Ok(body) => body
            .data
            .into_iter()
            .map(|m| {
                let label = m.id.clone();
                (m.id, label)
            })
            .collect(),
        Err(_) => vec![],
    }
}
