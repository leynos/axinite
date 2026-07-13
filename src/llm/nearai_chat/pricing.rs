//! Per-model pricing fetch from the NEAR AI `/v1/model/list` endpoint.
//!
//! Parses cost entries (`amount * 10^(-scale)`) into `Decimal` per-token
//! prices keyed by model ID and alias. Failures are non-fatal; callers fall
//! back to the static pricing table.

use std::collections::HashMap;

use reqwest::Client;
use rust_decimal::Decimal;
use rust_decimal::prelude::MathematicalOps;
use secrecy::ExposeSecret;
use serde::Deserialize;

use crate::llm::error::LlmError;
use crate::llm::session::SessionManager;

/// Cost amount from the NEAR AI `/v1/model/list` response.
///
/// Real cost per token = `amount * 10^(-scale)`.
#[derive(Debug, Deserialize)]
pub(super) struct ModelCost {
    pub(super) amount: f64,
    #[serde(default)]
    pub(super) scale: i32,
}

/// A single model entry from the pricing response.
#[derive(Debug, Deserialize)]
pub(super) struct PricingModelEntry {
    #[serde(default, alias = "modelId", alias = "model_id")]
    pub(super) model_id: Option<String>,
    #[serde(default, alias = "inputCostPerToken")]
    pub(super) input_cost_per_token: Option<ModelCost>,
    #[serde(default, alias = "outputCostPerToken")]
    pub(super) output_cost_per_token: Option<ModelCost>,
    #[serde(default)]
    pub(super) metadata: Option<PricingMetadata>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PricingMetadata {
    #[serde(default)]
    pub(super) aliases: Vec<String>,
}

/// Wrapper for the `/v1/model/list` response body.
#[derive(Debug, Deserialize)]
pub(super) struct PricingResponse {
    #[serde(default)]
    pub(super) models: Option<Vec<PricingModelEntry>>,
    #[serde(default)]
    pub(super) data: Option<Vec<PricingModelEntry>>,
}

/// Convert a `ModelCost` to a `Decimal` per-token price.
pub(super) fn model_cost_to_decimal(mc: &ModelCost) -> Option<Decimal> {
    if mc.amount == 0.0 {
        return Some(Decimal::ZERO);
    }
    // amount * 10^(-scale)
    let base = Decimal::try_from(mc.amount).ok()?;
    let factor = Decimal::TEN.checked_powi(-i64::from(mc.scale))?;
    base.checked_mul(factor)
}

/// Fetch pricing from the NEAR AI `/v1/model/list` endpoint.
///
/// Returns a map of model_id → (input_cost_per_token, output_cost_per_token).
/// Errors are non-fatal; callers should fall back to the static lookup table.
pub(super) async fn fetch_pricing(
    client: &Client,
    base_url: &str,
    api_key: Option<&secrecy::SecretString>,
    session: &SessionManager,
) -> Result<HashMap<String, (Decimal, Decimal)>, LlmError> {
    let base = base_url.trim_end_matches('/');
    let url = if base.ends_with("/v1") {
        format!("{}/model/list", base)
    } else {
        format!("{}/v1/model/list", base)
    };

    let token = if let Some(key) = api_key {
        key.expose_secret().to_string()
    } else {
        let tok = session.get_token().await?;
        tok.expose_secret().to_string()
    };

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| LlmError::RequestFailed {
            provider: "nearai_chat".to_string(),
            reason: format!("Failed to fetch pricing: {}", e),
        })?;

    if !response.status().is_success() {
        return Err(LlmError::RequestFailed {
            provider: "nearai_chat".to_string(),
            reason: format!("Pricing endpoint returned HTTP {}", response.status()),
        });
    }

    let body = response.text().await.map_err(|e| LlmError::RequestFailed {
        provider: "nearai_chat".to_string(),
        reason: format!("Failed to read pricing response: {}", e),
    })?;

    // Parse as {models: [...]} or {data: [...]} or direct array
    let entries: Vec<PricingModelEntry> =
        if let Ok(resp) = serde_json::from_str::<PricingResponse>(&body) {
            resp.models.or(resp.data).unwrap_or_default()
        } else if let Ok(arr) = serde_json::from_str::<Vec<PricingModelEntry>>(&body) {
            arr
        } else {
            return Ok(HashMap::new());
        };

    let mut map = HashMap::new();
    for entry in &entries {
        let (Some(input_mc), Some(output_mc)) =
            (&entry.input_cost_per_token, &entry.output_cost_per_token)
        else {
            continue;
        };
        let (Some(input), Some(output)) = (
            model_cost_to_decimal(input_mc),
            model_cost_to_decimal(output_mc),
        ) else {
            continue;
        };

        // Insert under the primary model_id
        if let Some(ref id) = entry.model_id {
            map.insert(id.clone(), (input, output));
        }
        // Also insert under any aliases
        if let Some(ref meta) = entry.metadata {
            for alias in &meta.aliases {
                map.insert(alias.clone(), (input, output));
            }
        }
    }

    Ok(map)
}
