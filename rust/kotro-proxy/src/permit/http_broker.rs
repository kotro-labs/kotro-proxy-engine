//! Host HTTP front for the thin broker (`POST /v1/broker/draft-pr`).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::broker::{handle_draft_pr, BrokerError, BrokerOptions, BrokerSession, DraftPrRequest};

#[derive(Clone)]
struct BrokerState {
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
    /// Abuse polish: max draft-pr attempts per rolling window.
    rate: Arc<RateWindow>,
}

struct RateWindow {
    max: u32,
    window: Duration,
    hits: Mutex<Vec<Instant>>,
    rejected: AtomicU32,
}

impl RateWindow {
    fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            hits: Mutex::new(Vec::new()),
            rejected: AtomicU32::new(0),
        }
    }

    fn allow(&self) -> bool {
        let now = Instant::now();
        let mut hits = self.hits.lock();
        hits.retain(|t| now.duration_since(*t) < self.window);
        if hits.len() as u32 >= self.max {
            self.rejected.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        hits.push(now);
        true
    }
}

fn new_state(
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
) -> BrokerState {
    BrokerState {
        session,
        allow_once_override,
        dry_run,
        rate: Arc::new(RateWindow::new(10, Duration::from_secs(60))),
    }
}

/// Serve broker on `127.0.0.1:0` (ephemeral). Returns listen addr + shutdown sender.
pub async fn serve_broker(
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
) -> Result<(SocketAddr, oneshot::Sender<()>), String> {
    serve_broker_bind("127.0.0.1:0", session, allow_once_override, dry_run).await
}

/// Serve broker on an explicit bind address.
pub async fn serve_broker_at(
    bind: &str,
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
) -> Result<SocketAddr, String> {
    let app = router(new_state(session, allow_once_override, dry_run));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    Ok(addr)
}

async fn serve_broker_bind(
    bind: &str,
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
) -> Result<(SocketAddr, oneshot::Sender<()>), String> {
    let app = router(new_state(session, allow_once_override, dry_run));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });
    Ok((addr, tx))
}

fn router(state: BrokerState) -> Router {
    Router::new()
        .route("/v1/broker/draft-pr", post(draft_pr_handler))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(Arc::new(state))
}

async fn draft_pr_handler(
    State(state): State<Arc<BrokerState>>,
    headers: HeaderMap,
    Json(req): Json<DraftPrRequest>,
) -> Result<Json<super::broker::DraftPrResponse>, (StatusCode, String)> {
    if !state.rate.allow() {
        return Err((StatusCode::TOO_MANY_REQUESTS, "rate_limited".into()));
    }
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth
        .strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .unwrap_or(auth)
        .trim();
    let opts = BrokerOptions {
        session: state.session.clone(),
        run_token: token.to_string(),
        allow_once_override: state.allow_once_override.clone(),
        dry_run: state.dry_run,
        interactive: false,
    };
    match handle_draft_pr(&opts, &req) {
        Ok(r) => Ok(Json(r)),
        Err(BrokerError::Unauthorized) => Err((StatusCode::UNAUTHORIZED, "unauthorized".into())),
        Err(BrokerError::PermitDenied(m)) => {
            Err((StatusCode::FORBIDDEN, format!("permit_denied: {m}")))
        }
        Err(BrokerError::AllowOnceRequired) => {
            Err((StatusCode::UNAUTHORIZED, "allow_once_required".into()))
        }
        Err(BrokerError::AllowOnceDenied) => {
            Err((StatusCode::FORBIDDEN, "allow_once_denied".into()))
        }
        Err(BrokerError::ArtifactMismatch) => {
            Err((StatusCode::CONFLICT, "artifact_mismatch".into()))
        }
        Err(BrokerError::Expired) => Err((StatusCode::UNAUTHORIZED, "expired".into())),
        Err(BrokerError::TokenConsumed) => Err((StatusCode::CONFLICT, "token_consumed".into())),
        Err(BrokerError::GithubUnconfigured(m)) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            format!("github_unconfigured: {m}"),
        )),
        Err(BrokerError::BaseMoved) => Err((StatusCode::CONFLICT, "base_moved".into())),
        Err(BrokerError::Msg(m)) => Err((StatusCode::BAD_REQUEST, m)),
    }
}
