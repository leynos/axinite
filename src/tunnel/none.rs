//! No-op tunnel for local-only access.

use anyhow::Result;

use crate::tunnel::NativeTunnel;

/// No-op tunnel, no external exposure. `public_url()` always returns `None`.
pub struct NoneTunnel;

impl NativeTunnel for NoneTunnel {
    fn name(&self) -> &str {
        "none"
    }

    async fn start<'a>(&'a self, local_host: &'a str, local_port: u16) -> Result<String> {
        Ok(format!("http://{local_host}:{local_port}"))
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }

    fn public_url(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_none() {
        assert_eq!(NativeTunnel::name(&NoneTunnel), "none");
    }

    #[tokio::test]
    async fn start_returns_local_url() {
        let url = NativeTunnel::start(&NoneTunnel, "127.0.0.1", 7788)
            .await
            .unwrap();
        assert_eq!(url, "http://127.0.0.1:7788");
    }

    #[tokio::test]
    async fn stop_is_noop() {
        assert!(NativeTunnel::stop(&NoneTunnel).await.is_ok());
    }

    #[tokio::test]
    async fn health_is_always_true() {
        assert!(NativeTunnel::health_check(&NoneTunnel).await);
    }

    #[test]
    fn public_url_is_always_none() {
        assert!(NativeTunnel::public_url(&NoneTunnel).is_none());
    }
}
