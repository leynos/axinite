//! Tests for the recording and replaying HTTP interceptors.

use super::super::*;

#[tokio::test]
async fn recording_http_interceptor_passes_through_and_records() {
    let interceptor = RecordingHttpInterceptor::new();

    let req = HttpExchangeRequest {
        method: "GET".to_string(),
        url: "https://example.com".to_string(),
        headers: Vec::new(),
        body: None,
    };

    // before_request should return None (pass through)
    assert!(
        NativeHttpInterceptor::before_request(&interceptor, &req)
            .await
            .is_none()
    );

    // after_response records the exchange
    let resp = HttpExchangeResponse {
        status: 200,
        headers: Vec::new(),
        body: "ok".to_string(),
    };
    NativeHttpInterceptor::after_response(&interceptor, &req, &resp).await;

    let exchanges = interceptor.take_exchanges().await;
    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].request.url, "https://example.com");
}

#[tokio::test]
async fn replaying_http_interceptor_returns_recorded_responses() {
    let exchanges = vec![HttpExchange {
        request: HttpExchangeRequest {
            method: "GET".to_string(),
            url: "https://api.example.com/data".to_string(),
            headers: Vec::new(),
            body: None,
        },
        response: HttpExchangeResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"items": []}"#.to_string(),
        },
    }];
    let interceptor = ReplayingHttpInterceptor::new(exchanges);

    // First request: returns recorded response
    let req = HttpExchangeRequest {
        method: "GET".to_string(),
        url: "https://api.example.com/data".to_string(),
        headers: Vec::new(),
        body: None,
    };
    let resp = NativeHttpInterceptor::before_request(&interceptor, &req)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, r#"{"items": []}"#);

    // Second request: no more exchanges → 599
    let resp = NativeHttpInterceptor::before_request(&interceptor, &req)
        .await
        .unwrap();
    assert_eq!(resp.status, 599);
}
