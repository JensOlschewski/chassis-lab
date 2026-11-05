use crate::store::{Failure, Store};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clab_license::{
    ActivateRequest, ErrorBody, ErrorCode, RegisterRequest, RegisterResponse, SeatRequest, Token,
    ValidateRequest,
};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const REGISTRATIONS_PER_HOUR: usize = 20;
const HOUR: u64 = 3600;
/// Above this many tracked addresses, idle ones are dropped.
const TRACKED_ADDRESSES: usize = 1024;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs()
}

/// Registrations per address in the last hour, kept in memory.
#[derive(Default)]
struct Limiter(HashMap<String, Vec<u64>>);

impl Limiter {
    fn allow(&mut self, address: &str, now: u64) -> bool {
        if self.0.len() > TRACKED_ADDRESSES {
            self.0
                .retain(|_, times| times.iter().any(|time| now - time < HOUR));
        }
        let times = self.0.entry(address.to_owned()).or_default();
        times.retain(|time| now - time < HOUR);
        if times.len() >= REGISTRATIONS_PER_HOUR {
            return false;
        }
        times.push(now);
        true
    }
}

struct App {
    store: Mutex<Store>,
    limiter: Mutex<Limiter>,
    open_registration: bool,
}

type Shared = Arc<App>;

pub fn router(store: Store, open_registration: bool) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/v1/register", post(register))
        .route("/v1/activate", post(activate))
        .route("/v1/validate", post(validate))
        .route("/v1/deactivate", post(deactivate))
        .with_state(Arc::new(App {
            store: Mutex::new(store),
            limiter: Mutex::default(),
            open_registration,
        }))
}

/// The address the proxy saw. Traefik appends it last, so a client cannot choose it.
fn client_ip(headers: &HeaderMap) -> Option<String> {
    let forwarded = headers.get("x-forwarded-for")?.to_str().ok()?;
    let last = forwarded.rsplit(',').next()?.trim();
    last.parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Self::Denied(code) => {
                let status = match code {
                    ErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
                    ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                    _ => StatusCode::FORBIDDEN,
                };
                (status, Json(ErrorBody { error: code })).into_response()
            }
            Self::Internal(error) => {
                eprintln!("database error: {error}");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

/// A public status page. It shows no licenses, names or counts.
async fn index(State(app): State<Shared>) -> Html<String> {
    let database_ok = app.store.lock().expect("store lock").list().is_ok();
    let (database, class) = if database_ok {
        ("ok", "ok")
    } else {
        ("unavailable", "bad")
    };
    let registration = if app.open_registration {
        "accepted"
    } else {
        "closed"
    };
    Html(
        include_str!("index.html")
            .replace("{{database_class}}", class)
            .replace("{{database}}", database)
            .replace("{{registration}}", registration)
            .replace("{{version}}", env!("CARGO_PKG_VERSION")),
    )
}

async fn register(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, Failure> {
    if !app.open_registration {
        return Err(ErrorCode::RegistrationClosed.into());
    }
    let ip = client_ip(&headers);
    let address = ip.as_deref().unwrap_or("unknown");
    if !app
        .limiter
        .lock()
        .expect("limiter lock")
        .allow(address, now())
    {
        return Err(ErrorCode::RateLimited.into());
    }
    let store = app.store.lock().expect("store lock");
    store.register(&request, ip.as_deref(), now()).map(Json)
}

async fn activate(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(request): Json<ActivateRequest>,
) -> Result<Json<Token>, Failure> {
    let store = app.store.lock().expect("store lock");
    store
        .activate(&request, client_ip(&headers).as_deref(), now())
        .map(Json)
}

async fn validate(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(request): Json<ValidateRequest>,
) -> Result<Json<Token>, Failure> {
    let store = app.store.lock().expect("store lock");
    store
        .validate(&request, client_ip(&headers).as_deref(), now())
        .map(Json)
}

async fn deactivate(
    State(app): State<Shared>,
    Json(request): Json<SeatRequest>,
) -> Result<StatusCode, Failure> {
    let store = app.store.lock().expect("store lock");
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

    fn store(signing: SigningKey) -> Store {
        let dir = std::env::temp_dir().join(format!(
            "clab-license-test-{}",
            clab_license::new_install_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir.join("licenses.db"), signing, 7).unwrap()
    }

    async fn post_json(app: &Router, path: &str, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
        send(app, path, body, None).await
    }

    async fn send(
        app: &Router,
        path: &str,
        body: serde_json::Value,
        forwarded: Option<&str>,
    ) -> (StatusCode, Vec<u8>) {
        let mut request = Request::post(path).header("content-type", "application/json");
        if let Some(value) = forwarded {
            request = request.header("x-forwarded-for", value);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
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

    fn registration(install_id: &str) -> serde_json::Value {
        serde_json::json!({"install_id": install_id, "fingerprint": "fp", "hostname": "pc", "version": "1.0"})
    }

    #[tokio::test]
    async fn activate_validate_and_deactivate_over_http() {
        let signing = SigningKey::from_bytes(&[4; 32]);
        let public = signing.verifying_key();
        let store = store(signing);
        let (_, key) = store.create("Ada").unwrap();
        let app = router(store, true);

        let body = serde_json::json!({
            "key": key, "install_id": "a", "fingerprint": "fp-a", "hostname": "pc", "version": "1.0"
        });
        let (status, bytes) = post_json(&app, "/v1/activate", body).await;
        assert_eq!(status, StatusCode::OK);
        let token: Token = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(verify(&public, &token).unwrap().licensee, "Ada");

        let other = serde_json::json!({
            "key": key, "install_id": "b", "fingerprint": "fp-b", "hostname": "pc2", "version": "1.0"
        });
        let (status, bytes) = post_json(&app, "/v1/activate", other).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            serde_json::from_slice::<ErrorBody>(&bytes).unwrap().error,
            ErrorCode::SeatInUse
        );

        let check =
            serde_json::json!({"key": key, "install_id": "a", "hostname": "pc", "version": "1.0"});
        assert_eq!(
            post_json(&app, "/v1/validate", check.clone()).await.0,
            StatusCode::OK
        );
        let seat = serde_json::json!({"key": key, "install_id": "a"});
        assert_eq!(
            post_json(&app, "/v1/deactivate", seat).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            post_json(&app, "/v1/validate", check).await.0,
            StatusCode::FORBIDDEN
        );

        let empty = serde_json::json!({
            "key": key, "install_id": "", "fingerprint": "fp-a", "hostname": "pc", "version": "1.0"
        });
        assert_eq!(
            post_json(&app, "/v1/activate", empty).await.0,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn registering_returns_a_token_and_a_key_that_validates() {
        let signing = SigningKey::from_bytes(&[4; 32]);
        let public = signing.verifying_key();
        let app = router(store(signing), true);
        let (status, bytes) = post_json(&app, "/v1/register", registration("a")).await;
        assert_eq!(status, StatusCode::OK);
        let registered: RegisterResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(verify(&public, &registered.token).unwrap().licensee, "pc");
        let check = serde_json::json!({"key": registered.key, "install_id": "a", "hostname": "pc", "version": "1.0"});
        assert_eq!(
            post_json(&app, "/v1/validate", check).await.0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn the_status_page_shows_state_but_no_licenses() {
        let store = store(SigningKey::from_bytes(&[4; 32]));
        store.create("Ada Secret").unwrap();
        let app = router(store, false);
        let response = app
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let page = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(page.contains("closed") && page.contains("running"));
        assert!(!page.contains("Ada Secret") && !page.contains("{{"));
    }

    #[tokio::test]
    async fn registration_can_be_closed() {
        let app = router(store(SigningKey::from_bytes(&[4; 32])), false);
        let (status, bytes) = post_json(&app, "/v1/register", registration("a")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let error = serde_json::from_slice::<ErrorBody>(&bytes).unwrap().error;
        assert_eq!(error, ErrorCode::RegistrationClosed);
    }

    #[tokio::test]
    async fn registrations_are_limited_per_forwarded_address() {
        let app = router(store(SigningKey::from_bytes(&[4; 32])), true);
        for index in 0..REGISTRATIONS_PER_HOUR {
            let body = serde_json::json!({
                "install_id": format!("i{index}"), "fingerprint": format!("fp{index}"),
                "hostname": "pc", "version": "1.0"
            });
            assert_eq!(
                send(&app, "/v1/register", body, Some("1.2.3.4")).await.0,
                StatusCode::OK
            );
        }
        let more = serde_json::json!({"install_id": "x", "fingerprint": "fpx", "hostname": "pc", "version": "1.0"});
        let (status, bytes) = send(&app, "/v1/register", more.clone(), Some("1.2.3.4")).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            serde_json::from_slice::<ErrorBody>(&bytes).unwrap().error,
            ErrorCode::RateLimited
        );
        assert_eq!(
            send(&app, "/v1/register", more, Some("5.6.7.8")).await.0,
            StatusCode::OK
        );
    }

    #[test]
    fn the_limiter_forgets_after_an_hour() {
        let mut limiter = Limiter::default();
        for _ in 0..REGISTRATIONS_PER_HOUR {
            assert!(limiter.allow("a", 100));
        }
        assert!(!limiter.allow("a", 100 + HOUR - 1));
        assert!(limiter.allow("a", 100 + HOUR));
    }

    #[test]
    fn the_client_address_is_the_one_the_proxy_appended() {
        let headers = |value: &str| {
            let mut map = HeaderMap::new();
            map.insert("x-forwarded-for", value.parse().unwrap());
            map
        };
        assert_eq!(
            client_ip(&headers("9.9.9.9, 1.2.3.4")).as_deref(),
            Some("1.2.3.4")
        );
        assert_eq!(client_ip(&headers("not an ip")), None);
        assert_eq!(client_ip(&HeaderMap::new()), None);
    }
}
