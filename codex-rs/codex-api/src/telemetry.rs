use crate::error::ApiError;
use codex_client::Request;
use codex_client::RequestTelemetry;
use codex_client::Response;
use codex_client::RetryPolicy;
use codex_client::StreamResponse;
use codex_client::TransportError;
use codex_client::run_with_retry;
use http::StatusCode;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::tungstenite::Message;

/// Generic telemetry.
pub trait SseTelemetry: Send + Sync {
    fn on_sse_bytes(&self, bytes: u64);

    fn on_sse_poll(
        &self,
        result: &Result<
            Option<
                Result<
                    eventsource_stream::Event,
                    eventsource_stream::EventStreamError<TransportError>,
                >,
            >,
            tokio::time::error::Elapsed,
        >,
        duration: Duration,
    );
}

/// Telemetry for Responses WebSocket transport.
pub trait WebsocketTelemetry: Send + Sync {
    fn on_ws_request(&self, duration: Duration, error: Option<&ApiError>, connection_reused: bool);

    fn on_ws_event(
        &self,
        result: &Result<Option<Result<Message, Error>>, ApiError>,
        duration: Duration,
    );
}

pub(crate) trait WithStatus {
    fn status(&self) -> StatusCode;

    fn response_body_bytes(&self) -> Option<u64> {
        None
    }
}

fn http_status(err: &TransportError) -> Option<StatusCode> {
    match err {
        TransportError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

impl WithStatus for Response {
    fn status(&self) -> StatusCode {
        self.status
    }

    fn response_body_bytes(&self) -> Option<u64> {
        Some(u64::try_from(self.body.len()).unwrap_or(u64::MAX))
    }
}

impl WithStatus for StreamResponse {
    fn status(&self) -> StatusCode {
        self.status
    }
}

pub(crate) async fn run_with_request_telemetry<T, F, Fut>(
    policy: RetryPolicy,
    telemetry: Option<Arc<dyn RequestTelemetry>>,
    make_request: impl FnMut() -> Request,
    send: F,
) -> Result<T, TransportError>
where
    T: WithStatus,
    F: Clone + Fn(Request) -> Fut,
    Fut: Future<Output = Result<T, TransportError>>,
{
    // Wraps `run_with_retry` to attach per-attempt request telemetry for both
    // unary and streaming HTTP calls.
    run_with_retry(policy, make_request, move |req, attempt| {
        let telemetry = telemetry.clone();
        let send = send.clone();
        async move {
            let req = req.into_prepared().map_err(TransportError::Build)?;
            let request_body_bytes = request_body_bytes(&req);
            let start = Instant::now();
            let result = send(req).await;
            if let Some(t) = telemetry.as_ref() {
                let (status, err) = match &result {
                    Ok(resp) => (Some(resp.status()), None),
                    Err(err) => (http_status(err), Some(err)),
                };
                t.on_request(
                    attempt,
                    status,
                    err,
                    start.elapsed(),
                    request_body_bytes,
                    result
                        .as_ref()
                        .ok()
                        .and_then(WithStatus::response_body_bytes),
                );
            }
            result
        }
    })
    .await
}

fn request_body_bytes(req: &Request) -> Option<u64> {
    match req.body.as_ref() {
        Some(codex_client::RequestBody::EncodedJson(body)) => {
            Some(u64::try_from(body.as_bytes().len()).unwrap_or(u64::MAX))
        }
        Some(codex_client::RequestBody::Raw(body)) => {
            Some(u64::try_from(body.len()).unwrap_or(u64::MAX))
        }
        Some(codex_client::RequestBody::Json(_)) | None => None,
    }
}
