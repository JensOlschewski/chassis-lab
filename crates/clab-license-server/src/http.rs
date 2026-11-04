use crate::store::{Failure, Store};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use clab_license::{ActivateRequest, ErrorBody, ErrorCode, SeatRequest, Token};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

type Shared = Arc<Mutex<Store>>;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs()
}

pub fn router(store: Store) -> Router {
    Router::new()
        .route("/v1/activate", post(activate))
        .route("/v1/validate", post(validate))
        .route("/v1/deactivate", post(deactivate))
        .with_state(Arc::new(Mutex::new(store)))
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Self::Denied(ErrorCode::InvalidRequest) => (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: ErrorCode::InvalidRequest,
                }),
            )
                .into_response(),
            Self::Denied(code) => {
                (StatusCode::FORBIDDEN, Json(ErrorBody { error: code })).into_response()
            }
            Self::Internal(error) => {
                eprintln!("database error: {error}");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

async fn activate(
    State(store): State<Shared>,
    Json(request): Json<ActivateRequest>,
) -> Result<Json<Token>, Failure> {
    let store = store.lock().expect("store lock");
    store
        .activate(&request.key, &request.install_id, &request.hostname, now())
        .map(Json)
}

async fn validate(
    State(store): State<Shared>,
    Json(request): Json<SeatRequest>,
) -> Result<Json<Token>, Failure> {
    let store = store.lock().expect("store lock");
    store
        .validate(&request.key, &request.install_id, now())
        .map(Json)
}

async fn deactivate(
    State(store): State<Shared>,
    Json(request): Json<SeatRequest>,
) -> Result<StatusCode, Failure> {
    let store = store.lock().expect("store lock");
    store.deactivate(&request.key, &request.install_id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use clab_license::ed25519_dalek::SigningKey;
    use clab_license::verify;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn post_json(app: &Router, path: &str, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
        let request = Request::post(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        (
            status,
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
    }

    #[tokio::test]
    async fn activate_validate_and_deactivate_over_http() {
        let signing = SigningKey::from_bytes(&[4; 32]);
        let public = signing.verifying_key();
        let dir = tempfile_path();
        let store = Store::open(&dir, signing, 7).unwrap();
        let (_, key) = store.create("Ada").unwrap();
        let app = router(store);

        let body = serde_json::json!({"key": key, "install_id": "a", "hostname": "pc"});
        let (status, bytes) = post_json(&app, "/v1/activate", body).await;
        assert_eq!(status, StatusCode::OK);
        let token: Token = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(verify(&public, &token).unwrap().licensee, "Ada");

        let other = serde_json::json!({"key": key, "install_id": "b", "hostname": "pc2"});
        let (status, bytes) = post_json(&app, "/v1/activate", other).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            serde_json::from_slice::<ErrorBody>(&bytes).unwrap().error,
            ErrorCode::SeatInUse
        );

        let seat = serde_json::json!({"key": key, "install_id": "a"});
        assert_eq!(
            post_json(&app, "/v1/validate", seat.clone()).await.0,
            StatusCode::OK
        );
        assert_eq!(
            post_json(&app, "/v1/deactivate", seat.clone()).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            post_json(&app, "/v1/validate", seat).await.0,
            StatusCode::FORBIDDEN
        );

        let empty = serde_json::json!({"key": key, "install_id": "", "hostname": "pc"});
        assert_eq!(
            post_json(&app, "/v1/activate", empty).await.0,
            StatusCode::BAD_REQUEST
        );
    }

    fn tempfile_path() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "clab-license-test-{}",
            clab_license::new_install_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("licenses.db")
    }
}
