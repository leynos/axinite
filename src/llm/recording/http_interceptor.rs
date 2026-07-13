//! HTTP interception for trace recording and replay.
//!
//! Recording captures real request/response pairs; replay short-circuits
//! requests with previously recorded responses.

use core::future::Future;
use core::pin::Pin;
use std::collections::VecDeque;

use tokio::sync::Mutex;

use super::trace_format::{HttpExchange, HttpExchangeRequest, HttpExchangeResponse};

/// Boxed future used at the dyn HTTP-interceptor boundary.
pub type HttpInterceptorFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for intercepting HTTP requests from tools.
///
/// During recording, the interceptor captures exchanges after the real
/// request completes. During replay, it short-circuits with a recorded response.
pub trait HttpInterceptor: Send + Sync + std::fmt::Debug {
    /// Called before making an HTTP request.
    ///
    /// Return `Some(response)` to short-circuit (replay mode).
    /// Return `None` to let the real request proceed (recording mode).
    fn before_request<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
    ) -> HttpInterceptorFuture<'a, Option<HttpExchangeResponse>>;

    /// Called after a real HTTP request completes (recording mode only).
    fn after_response<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
        response: &'a HttpExchangeResponse,
    ) -> HttpInterceptorFuture<'a, ()>;
}

/// Native async sibling trait for concrete HTTP-interceptor implementations.
pub trait NativeHttpInterceptor: Send + Sync + std::fmt::Debug {
    /// See [`HttpInterceptor::before_request`].
    fn before_request<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
    ) -> impl Future<Output = Option<HttpExchangeResponse>> + Send + 'a;

    /// See [`HttpInterceptor::after_response`].
    fn after_response<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
        response: &'a HttpExchangeResponse,
    ) -> impl Future<Output = ()> + Send + 'a;
}

impl<T> HttpInterceptor for T
where
    T: NativeHttpInterceptor + Send + Sync + std::fmt::Debug,
{
    fn before_request<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
    ) -> HttpInterceptorFuture<'a, Option<HttpExchangeResponse>> {
        Box::pin(NativeHttpInterceptor::before_request(self, request))
    }

    fn after_response<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
        response: &'a HttpExchangeResponse,
    ) -> HttpInterceptorFuture<'a, ()> {
        Box::pin(NativeHttpInterceptor::after_response(
            self, request, response,
        ))
    }
}

/// Records HTTP exchanges during a live session.
#[derive(Debug)]
pub struct RecordingHttpInterceptor {
    exchanges: Mutex<Vec<HttpExchange>>,
}

impl Default for RecordingHttpInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingHttpInterceptor {
    pub fn new() -> Self {
        Self {
            exchanges: Mutex::new(Vec::new()),
        }
    }

    /// Return all recorded exchanges.
    pub async fn take_exchanges(&self) -> Vec<HttpExchange> {
        self.exchanges.lock().await.clone()
    }
}

impl NativeHttpInterceptor for RecordingHttpInterceptor {
    async fn before_request<'a>(
        &'a self,
        _request: &'a HttpExchangeRequest,
    ) -> Option<HttpExchangeResponse> {
        // Recording mode: let the real request proceed
        None
    }

    async fn after_response<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
        response: &'a HttpExchangeResponse,
    ) {
        self.exchanges.lock().await.push(HttpExchange {
            request: request.clone(),
            response: response.clone(),
        });
    }
}

/// Replays recorded HTTP exchanges during test runs.
///
/// Returns responses in order. If more requests arrive than recorded
/// exchanges, returns a 599 error response.
#[derive(Debug)]
pub struct ReplayingHttpInterceptor {
    exchanges: Mutex<VecDeque<HttpExchange>>,
}

impl ReplayingHttpInterceptor {
    pub fn new(exchanges: Vec<HttpExchange>) -> Self {
        Self {
            exchanges: Mutex::new(VecDeque::from(exchanges)),
        }
    }
}

impl NativeHttpInterceptor for ReplayingHttpInterceptor {
    async fn before_request<'a>(
        &'a self,
        request: &'a HttpExchangeRequest,
    ) -> Option<HttpExchangeResponse> {
        let mut queue = self.exchanges.lock().await;
        if let Some(exchange) = queue.pop_front() {
            // Soft-check: warn if the request doesn't match
            if exchange.request.url != request.url || exchange.request.method != request.method {
                tracing::warn!(
                    expected_url = %exchange.request.url,
                    actual_url = %request.url,
                    expected_method = %exchange.request.method,
                    actual_method = %request.method,
                    "HTTP replay: request mismatch (returning recorded response anyway)"
                );
            }
            Some(exchange.response)
        } else {
            tracing::error!(
                url = %request.url,
                method = %request.method,
                "HTTP replay: no more recorded exchanges, returning error"
            );
            Some(HttpExchangeResponse {
                status: 599,
                headers: Vec::new(),
                body: "trace replay: no more recorded HTTP exchanges".to_string(),
            })
        }
    }

    async fn after_response<'a>(
        &'a self,
        _request: &'a HttpExchangeRequest,
        _response: &'a HttpExchangeResponse,
    ) {
        // Replay mode: nothing to record
    }
}
