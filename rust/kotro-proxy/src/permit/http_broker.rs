//! Host HTTP front for the thin broker (`POST /v1/broker/draft-pr`).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use tokio::sync::oneshot;

use super::broker::{handle_draft_pr, BrokerError, BrokerOptions, BrokerSession, DraftPrRequest};

#[derive(Clone)]
struct BrokerState {
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
}

/// Serve broker on `127.0.0.1:0` (ephemeral). Returns listen addr + shutdown sender.
pub async fn serve_broker(
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
) -> Result<(SocketAddr, oneshot::Sender<()>), String> {
    serve_broker_bind("127.0.0.1:0", session, allow_once_override, dry_run).await
}

/// Serve broker on an explicit bind address; blocks until Ctrl-C when used from CLI via
/// [`serve_broker_at`] (no oneshot). Prefer [`serve_broker`] in tests.
pub async fn serve_broker_at(
    bind: &str,
    session: BrokerSession,
    allow_once_override: Option<String>,
    dry_run: bool,
) -> Result<SocketAddr, String> {
    let state = BrokerState {
        session,
        allow_once_override,
        dry_run,
    };
    let app = Router::new()
        .route("/v1/broker/draft-pr", post(draft_pr_handler))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(Arc::new(state));

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    // Spawn and return immediately so the CLI can print the listen URL then wait on ctrl_c.
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
    let state = BrokerState {
        session,
        allow_once_override,
        dry_run,
    };
    let app = Router::new()
        .route("/v1/broker/draft-pr", post(draft_pr_handler))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(Arc::new(state));

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

async fn draft_pr_handler(
    State(state): State<Arc<BrokerState>>,
    headers: HeaderMap,
    Json(req): Json<DraftPrRequest>,
) -> Result<Json<super::broker::DraftPrResponse>, (StatusCode, String)> {
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
        Err(BrokerError::AllowOnceRequired) => Err((
            StatusCode::UNAUTHORIZED,
            "allow_once_required".into(),
        )),
        Err(BrokerError::AllowOnceDenied) => {
            Err((StatusCode::FORBIDDEN, "allow_once_denied".into()))
        }
        Err(BrokerError::ArtifactMismatch) => {
            Err((StatusCode::CONFLICT, "artifact_mismatch".into()))
        }
        Err(BrokerError::Expired) => Err((StatusCode::UNAUTHORIZED, "expired".into())),
        Err(BrokerError::GithubUnconfigured(m)) => {
            Err((StatusCode::SERVICE_UNAVAILABLE, format!("github_unconfigured: {m}")))
        }
        Err(BrokerError::BaseMoved) => Err((StatusCode::CONFLICT, "base_moved".into())),
        Err(BrokerError::Msg(m)) => Err((StatusCode::BAD_REQUEST, m)),
    }
}
