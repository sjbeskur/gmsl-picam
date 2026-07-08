use crate::frame_store::FrameStore;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::StreamExt;

pub const MJPEG_BOUNDARY: &str = "iris";

pub fn router(store: Arc<FrameStore>) -> Router {
    Router::new()
        .route("/status", get(status_handler))
        .route("/frame.jpg", get(frame_handler))
        .route("/stream.mjpg", get(stream_handler))
        .with_state(store)
}

async fn status_handler(
    State(store): State<Arc<FrameStore>>,
) -> Json<crate::frame_store::ServiceStatus> {
    Json(store.status())
}

async fn frame_handler(State(store): State<Arc<FrameStore>>) -> Response {
    let Some(frame) = store.latest_frame() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"))],
        frame.jpeg.as_ref().to_vec(),
    )
        .into_response()
}

async fn stream_handler(State(_store): State<Arc<FrameStore>>) -> Response {
    let store = _store;
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_millis(100))).filter_map(
        move |_| {
            store
                .latest_frame()
                .map(|frame| Ok::<Vec<u8>, Infallible>(multipart_chunk(&frame.jpeg)))
        },
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/x-mixed-replace; boundary={MJPEG_BOUNDARY}"),
        )
        .body(axum::body::Body::from_stream(stream))
        .expect("valid MJPEG response")
}

fn multipart_chunk(jpeg: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(jpeg.len() + 128);
    chunk.extend_from_slice(b"--");
    chunk.extend_from_slice(MJPEG_BOUNDARY.as_bytes());
    chunk.extend_from_slice(b"\r\nContent-Type: image/jpeg\r\nContent-Length: ");
    chunk.extend_from_slice(jpeg.len().to_string().as_bytes());
    chunk.extend_from_slice(b"\r\n\r\n");
    chunk.extend_from_slice(jpeg);
    chunk.extend_from_slice(b"\r\n");
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_store::{CameraState, FrameMetadata};
    fn sample_metadata() -> FrameMetadata {
        FrameMetadata {
            frame_index: 1,
            sensor_ts_ns: 1_000_000_000,
            exposure_us: 8_000,
            analogue_gain: 1.5,
            coi_ns: 1_004_000_000,
            width: 1_280,
            height: 720,
            pixel_format: "NV12".to_string(),
            captured_at_unix_ms: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn status_handler_returns_json() {
        let store = Arc::new(FrameStore::new(CameraState::Running, 1_280, 720, "NV12"));
        store.update_frame(vec![1, 2, 3], sample_metadata());

        let response = status_handler(State(store)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let status: crate::frame_store::ServiceStatus =
            serde_json::from_slice(&body).expect("json");
        assert_eq!(status.frame_count, 1);
        assert_eq!(
            status.latest_frame.as_ref().map(|frame| frame.frame_index),
            Some(1)
        );
    }

    #[tokio::test]
    async fn frame_handler_returns_latest_jpeg() {
        let store = Arc::new(FrameStore::new(CameraState::Running, 1_280, 720, "NV12"));
        store.update_frame(vec![9, 8, 7], sample_metadata());

        let response = frame_handler(State(store)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("image/jpeg")
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(body.as_ref(), &[9, 8, 7]);
    }

    #[tokio::test]
    async fn frame_handler_returns_503_when_empty() {
        let store = Arc::new(FrameStore::new(CameraState::Starting, 1_280, 720, "NV12"));

        let response = frame_handler(State(store)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn stream_handler_sets_mjpeg_content_type() {
        let store = Arc::new(FrameStore::new(CameraState::Running, 1_280, 720, "NV12"));

        let response = stream_handler(State(store)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("multipart/x-mixed-replace; boundary=iris")
        );
    }
}
