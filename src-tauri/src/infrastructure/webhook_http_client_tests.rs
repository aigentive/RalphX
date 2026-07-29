#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::infrastructure::webhook_http_client::{
        HyperWebhookClient, MockWebhookHttpClient, WebhookDeliveryError, WebhookHttpClient,
    };

    #[tokio::test]
    async fn mock_records_call_and_returns_status() {
        let client = MockWebhookHttpClient::new(200);
        let body = br#"{"event":"test"}"#.to_vec();
        let mut headers = HashMap::new();
        headers.insert("X-Signature".to_string(), "sha256=abc".to_string());

        let status = client
            .post("http://example.com/webhook", body.clone(), headers.clone())
            .await
            .unwrap();

        assert_eq!(status, 200);
        assert_eq!(client.call_count(), 1);

        let calls = client.calls.lock().unwrap();
        assert_eq!(calls[0].url, "http://example.com/webhook");
        assert_eq!(calls[0].body, body);
        assert_eq!(calls[0].headers.get("X-Signature").unwrap(), "sha256=abc");
    }

    #[tokio::test]
    async fn mock_with_error_returns_delivery_error() {
        let client = MockWebhookHttpClient::with_error();
        let result = client
            .post("http://example.com/webhook", vec![], HashMap::new())
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WebhookDeliveryError::Request(_)));
        // Still records the call even on error
        assert_eq!(client.call_count(), 1);
    }

    #[tokio::test]
    async fn mock_custom_status_code() {
        let client = MockWebhookHttpClient::new(503);
        let status = client
            .post("http://example.com/webhook", vec![], HashMap::new())
            .await
            .unwrap();
        assert_eq!(status, 503);
    }

    #[tokio::test]
    async fn mock_multiple_calls_all_recorded() {
        let client = MockWebhookHttpClient::new(200);
        for _ in 0..3 {
            client
                .post("http://example.com/webhook", vec![], HashMap::new())
                .await
                .unwrap();
        }
        assert_eq!(client.call_count(), 3);
    }

    #[test]
    fn hyper_client_default_constructs() {
        // Verify HyperWebhookClient::new() doesn't panic outside async context.
        // The client itself is constructed synchronously; only actual HTTP requests need tokio.
        let _client = HyperWebhookClient::new();
    }

    #[test]
    fn webhook_delivery_error_display() {
        assert_eq!(
            WebhookDeliveryError::Request("timeout".to_string()).to_string(),
            "HTTP request failed: timeout"
        );
        assert_eq!(
            WebhookDeliveryError::Timeout(30).to_string(),
            "Delivery timed out after 30s"
        );
        assert_eq!(
            WebhookDeliveryError::InvalidUrl("bad://url".to_string()).to_string(),
            "Invalid URL: bad://url"
        );
    }
}
