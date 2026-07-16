//! Unit tests for tunnel provider creation and error reporting.

use super::*;
use tokio::process::Command;

fn assert_tunnel_err(cfg: &TunnelProviderConfig, needle: &str) {
    match create_tunnel(cfg) {
        Err(e) => assert!(
            e.to_string().contains(needle),
            "Expected error containing \"{needle}\", got: {e}"
        ),
        Ok(_) => panic!("Expected error containing \"{needle}\", but got Ok"),
    }
}

#[test]
fn factory_none_returns_none() {
    let cfg = TunnelProviderConfig::default();
    assert!(create_tunnel(&cfg).unwrap().is_none());
}

#[test]
fn factory_empty_returns_none() {
    let cfg = TunnelProviderConfig {
        provider: String::new(),
        ..Default::default()
    };
    assert!(create_tunnel(&cfg).unwrap().is_none());
}

#[test]
fn factory_unknown_provider_errors() {
    let cfg = TunnelProviderConfig {
        provider: "wireguard".into(),
        ..Default::default()
    };
    assert_tunnel_err(&cfg, "Unknown tunnel provider");
}

#[test]
fn factory_cloudflare_missing_config_errors() {
    let cfg = TunnelProviderConfig {
        provider: "cloudflare".into(),
        ..Default::default()
    };
    assert_tunnel_err(&cfg, "TUNNEL_CF_TOKEN");
}

#[test]
fn factory_cloudflare_with_config_ok() {
    use crate::testing::credentials::TEST_BEARER_TOKEN;
    let cfg = TunnelProviderConfig {
        provider: "cloudflare".into(),
        cloudflare: Some(CloudflareTunnelConfig {
            token: TEST_BEARER_TOKEN.into(),
        }),
        ..Default::default()
    };
    let t = create_tunnel(&cfg).unwrap().unwrap();
    assert_eq!(t.name(), "cloudflare");
}

#[test]
fn factory_tailscale_defaults_ok() {
    let cfg = TunnelProviderConfig {
        provider: "tailscale".into(),
        ..Default::default()
    };
    let t = create_tunnel(&cfg).unwrap().unwrap();
    assert_eq!(t.name(), "tailscale");
}

#[test]
fn factory_ngrok_missing_config_errors() {
    let cfg = TunnelProviderConfig {
        provider: "ngrok".into(),
        ..Default::default()
    };
    assert_tunnel_err(&cfg, "TUNNEL_NGROK_TOKEN");
}

#[test]
fn factory_ngrok_with_config_ok() {
    let cfg = TunnelProviderConfig {
        provider: "ngrok".into(),
        ngrok: Some(NgrokTunnelConfig {
            auth_token: "tok".into(),
            domain: None,
        }),
        ..Default::default()
    };
    let t = create_tunnel(&cfg).unwrap().unwrap();
    assert_eq!(t.name(), "ngrok");
}

#[test]
fn factory_custom_missing_config_errors() {
    let cfg = TunnelProviderConfig {
        provider: "custom".into(),
        ..Default::default()
    };
    assert_tunnel_err(&cfg, "TUNNEL_CUSTOM_COMMAND");
}

#[test]
fn factory_custom_with_config_ok() {
    let cfg = TunnelProviderConfig {
        provider: "custom".into(),
        custom: Some(CustomTunnelConfig {
            start_command: "echo tunnel".into(),
            health_url: None,
            url_pattern: None,
        }),
        ..Default::default()
    };
    let t = create_tunnel(&cfg).unwrap().unwrap();
    assert_eq!(t.name(), "custom");
}

#[tokio::test]
async fn kill_shared_no_process_is_ok() {
    let proc = new_shared_process();
    assert!(kill_shared(&proc).await.is_ok());
    assert!(proc.lock().await.is_none());
}

#[tokio::test]
async fn kill_shared_terminates_child() {
    let proc = new_shared_process();

    let child = Command::new("sleep")
        .arg("30")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("sleep should spawn");

    {
        let mut guard = proc.lock().await;
        *guard = Some(TunnelProcess { child });
    }

    kill_shared(&proc).await.unwrap();
    assert!(proc.lock().await.is_none());
}
