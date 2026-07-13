//! Tests for pricing deserialization and per-token cost resolution.

use rust_decimal_macros::dec;

use super::super::pricing::*;
use super::super::*;
use super::{test_nearai_config, test_session};
use crate::llm::costs;
use crate::llm::provider::NativeLlmProvider;

#[test]
fn test_model_cost_to_decimal_basic() {
    // amount=3, scale=6 → 3 * 10^-6 = 0.000003
    let mc = ModelCost {
        amount: 3.0,
        scale: 6,
    };
    let result = model_cost_to_decimal(&mc).unwrap();
    assert_eq!(result, dec!(0.000003));
}

#[test]
fn test_model_cost_to_decimal_zero() {
    let mc = ModelCost {
        amount: 0.0,
        scale: 6,
    };
    assert_eq!(model_cost_to_decimal(&mc), Some(Decimal::ZERO));
}

#[test]
fn test_model_cost_to_decimal_larger_scale() {
    // amount=85, scale=8 → 85 * 10^-8 = 0.00000085
    let mc = ModelCost {
        amount: 85.0,
        scale: 8,
    };
    let result = model_cost_to_decimal(&mc).unwrap();
    assert_eq!(result, dec!(0.00000085));
}

#[test]
fn test_cost_per_token_uses_pricing_map() {
    let cfg = test_nearai_config("http://127.0.0.1:8318");
    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");

    // Inject pricing directly
    {
        let mut guard = provider.pricing.write().unwrap();
        guard.insert("test-model".to_string(), (dec!(0.000001), dec!(0.000005)));
    }

    let (input, output) = provider.cost_per_token();
    assert_eq!(input, dec!(0.000001));
    assert_eq!(output, dec!(0.000005));
}

#[test]
fn test_cost_per_token_falls_back_to_static() {
    let mut cfg = test_nearai_config("http://127.0.0.1:8318");
    cfg.model = "gpt-4o".to_string();
    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");

    // No pricing in map, should fall back to static costs::model_cost
    let (input, output) = provider.cost_per_token();
    let (expected_in, expected_out) = costs::model_cost("gpt-4o").unwrap();
    assert_eq!(input, expected_in);
    assert_eq!(output, expected_out);
}

#[test]
fn test_cost_per_token_falls_back_to_default() {
    let mut cfg = test_nearai_config("http://127.0.0.1:8318");
    cfg.model = "some-unknown-nearai-model".to_string();
    let provider = NearAiChatProvider::new(cfg, test_session()).expect("provider");

    // No pricing in map, not in static table, should use default_cost
    let (input, output) = provider.cost_per_token();
    let (default_in, default_out) = costs::default_cost();
    assert_eq!(input, default_in);
    assert_eq!(output, default_out);
}

// -- Pricing types deserialization ----------------------------------------

#[test]
fn test_model_cost_deserialize() {
    let json = r#"{"amount": 3.0, "scale": 6}"#;
    let mc: ModelCost = serde_json::from_str(json).unwrap();
    assert_eq!(mc.amount, 3.0);
    assert_eq!(mc.scale, 6);
}

#[test]
fn test_model_cost_scale_defaults_to_zero() {
    let json = r#"{"amount": 0.5}"#;
    let mc: ModelCost = serde_json::from_str(json).unwrap();
    assert_eq!(mc.scale, 0);
}

#[test]
fn test_model_cost_to_decimal_negative_scale() {
    // amount=2, scale=-3 → 2 * 10^3 = 2000
    let mc = ModelCost {
        amount: 2.0,
        scale: -3,
    };
    let result = model_cost_to_decimal(&mc).unwrap();
    assert_eq!(result, dec!(2000));
}

#[test]
fn test_pricing_model_entry_deserialize_camel_case_aliases() {
    let json = serde_json::json!({
        "modelId": "claude-3-5-sonnet",
        "inputCostPerToken": {"amount": 3.0, "scale": 6},
        "outputCostPerToken": {"amount": 15.0, "scale": 6},
        "metadata": {"aliases": ["claude-sonnet", "claude-3.5-sonnet"]}
    });
    let entry: PricingModelEntry = serde_json::from_value(json).unwrap();
    assert_eq!(entry.model_id, Some("claude-3-5-sonnet".to_string()));
    let input = model_cost_to_decimal(entry.input_cost_per_token.as_ref().unwrap()).unwrap();
    assert_eq!(input, dec!(0.000003));
    let output = model_cost_to_decimal(entry.output_cost_per_token.as_ref().unwrap()).unwrap();
    assert_eq!(output, dec!(0.000015));
    assert_eq!(
        entry.metadata.unwrap().aliases,
        vec!["claude-sonnet", "claude-3.5-sonnet"]
    );
}

#[test]
fn test_pricing_model_entry_deserialize_snake_case() {
    let json = serde_json::json!({
        "model_id": "gpt-4o",
        "input_cost_per_token": {"amount": 5.0, "scale": 6},
        "output_cost_per_token": {"amount": 15.0, "scale": 6}
    });
    let entry: PricingModelEntry = serde_json::from_value(json).unwrap();
    assert_eq!(entry.model_id, Some("gpt-4o".to_string()));
    assert!(entry.input_cost_per_token.is_some());
    assert!(entry.metadata.is_none());
}

#[test]
fn test_pricing_response_models_wrapper() {
    let json = serde_json::json!({
        "models": [
            {"model_id": "m1", "input_cost_per_token": {"amount": 1.0, "scale": 6},
             "output_cost_per_token": {"amount": 2.0, "scale": 6}}
        ]
    });
    let resp: PricingResponse = serde_json::from_value(json).unwrap();
    assert!(resp.models.is_some());
    assert_eq!(resp.models.unwrap().len(), 1);
    assert!(resp.data.is_none());
}

#[test]
fn test_pricing_response_data_wrapper() {
    let json = serde_json::json!({
        "data": [
            {"model_id": "m1"},
            {"model_id": "m2"}
        ]
    });
    let resp: PricingResponse = serde_json::from_value(json).unwrap();
    assert!(resp.models.is_none());
    assert_eq!(resp.data.unwrap().len(), 2);
}
