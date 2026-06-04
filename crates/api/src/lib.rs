// Axum HTTP server. Endpoints:
//   GET /health   → liveness
//   GET /stats    → aggregated trade counts + lifetime + 24h profit
//   GET /routes   → snapshot of dynamic route registry
//   GET /trades   → most recent 50 trades
//   GET /metrics  → JSON observability summary + per-RPC-URL health.
//                   This is the operator's one-stop "is the bot doing work?"
//                   endpoint — scraped by uptime monitors, eyeballed in a
//                   browser, also drives the Telegram /digest screen.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;

use chain::HttpPool;
use observability::ReplayBuffer;
use routes::DynamicRouteRegistry;
use storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<Storage>,
    pub routes: Arc<DynamicRouteRegistry>,
    /// Optional — present once the Executor has been constructed. /metrics
    /// returns an empty observability block when None (e.g. during very
    /// early boot before the executor task starts).
    pub observ: Option<Arc<ReplayBuffer>>,
    /// Optional — surfaces per-RPC-URL health to /metrics. Same lifecycle
    /// caveat as `observ`.
    pub rpc_pool: Option<HttpPool>,
}

pub async fn run(state: AppState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/routes", get(routes_handler))
        .route("/trades", get(trades))
        .route("/metrics", get(metrics))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("api listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({
        "status": "alive",
        "ts": chrono::Utc::now().to_rfc3339(),
    })))
}

async fn stats(State(s): State<AppState>) -> impl IntoResponse {
    match s.storage.aggregate_stats().await {
        Ok(stats) => (StatusCode::OK, Json(json!({
            "by_kind": stats.by_kind,
            "total_profit_all_time": stats.total_profit_all_time,
            "total_profit_last_24h": stats.total_profit_last_24h,
            "dynamic_routes_active": s.routes.count().await,
        }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
    }
}

async fn routes_handler(State(s): State<AppState>) -> impl IntoResponse {
    let snap = s.routes.snapshot().await;
    (StatusCode::OK, Json(json!({
        "count": snap.len(),
        "routes": snap.iter().map(|r| json!({
            "id": r.id,
            "num_hops": r.num_hops,
            "token_a": format!("{:?}", r.token_a),
            "token_b": format!("{:?}", r.token_b),
            "token_c": format!("{:?}", r.token_c),
            "dex1": r.dex1, "dex2": r.dex2, "dex3": r.dex3,
            "fee1": r.fee1, "fee2": r.fee2, "fee3": r.fee3,
        })).collect::<Vec<_>>(),
    })))
}

async fn trades(State(s): State<AppState>) -> impl IntoResponse {
    match s.storage.recent_trades(50).await {
        Ok(rows) => (StatusCode::OK, Json(json!({"trades": rows}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
    }
}

/// Combined operator dashboard: observability counters + per-RPC health.
/// Returned as a single JSON object so a `curl /metrics | jq` lands on the
/// 90% answer to "is the bot doing work?".
async fn metrics(State(s): State<AppState>) -> impl IntoResponse {
    let observ_block = match s.observ.as_ref() {
        Some(buf) => {
            let summary = buf.summary();
            json!({
                "events_recorded": buf.len(),
                "buffer_capacity": buf.capacity(),
                "total": summary.total,
                "sim_passed": summary.sim_passed,
                "sim_failed": summary.sim_failed,
                "success": summary.success,
                "reverted": summary.reverted,
                "gas_rejected": summary.gas_rejected,
                "success_rate": summary.success_rate(),
                "mean_latency_ms": summary.mean_latency_ms,
                "realized_profit_usdc": summary.total_realized_profit_usdc,
            })
        }
        None => json!({"status": "unavailable"}),
    };

    let rpc_block = match s.rpc_pool.as_ref() {
        Some(pool) => {
            let endpoints: Vec<_> = pool.status().into_iter().map(|s| json!({
                "index": s.index,
                // Sanitize URL: keep scheme+host but DROP path/query.
                // Alchemy/drpc/Infura/QuickNode embed API keys in the path
                // (e.g. https://eth-mainnet.alchemyapi.io/v2/<KEY>) or
                // query (?apikey=<KEY>). Serializing the raw URL leaks
                // credentials to anyone who can curl :3001/metrics.
                "host": sanitize_url(&s.url),
                "healthy": s.healthy,
                "failures": s.failures,
                "quota_blocked": s.quota_blocked,
            })).collect();
            json!({
                "endpoint_count": endpoints.len(),
                "endpoints": endpoints,
            })
        }
        None => json!({"status": "unavailable"}),
    };

    (StatusCode::OK, Json(json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "observability": observ_block,
        "rpc": rpc_block,
    })))
}

/// Strip path + query from a URL, leaving only `scheme://host[:port]`.
/// Used by /metrics to expose RPC endpoint health without leaking the
/// API key embedded in the path (most paid RPC providers — Alchemy,
/// drpc, Infura, QuickNode — put the key in the URL path or query).
fn sanitize_url(url: &url::Url) -> String {
    let scheme = url.scheme();
    let host = url.host_str().unwrap_or("?");
    match url.port() {
        Some(p) => format!("{scheme}://{host}:{p}"),
        None => format!("{scheme}://{host}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_alchemy_api_key() {
        let u: url::Url = "https://base-mainnet.g.alchemy.com/v2/SECRET_KEY_12345".parse().unwrap();
        assert_eq!(sanitize_url(&u), "https://base-mainnet.g.alchemy.com");
    }

    #[test]
    fn sanitize_strips_query_api_key() {
        let u: url::Url = "https://rpc.example.com/?apikey=SECRET".parse().unwrap();
        assert_eq!(sanitize_url(&u), "https://rpc.example.com");
    }

    #[test]
    fn sanitize_preserves_port() {
        let u: url::Url = "https://node.local:8545/path/SECRET".parse().unwrap();
        assert_eq!(sanitize_url(&u), "https://node.local:8545");
    }

    #[test]
    fn sanitize_handles_ws_scheme() {
        let u: url::Url = "wss://ws.example.com/v2/KEY".parse().unwrap();
        assert_eq!(sanitize_url(&u), "wss://ws.example.com");
    }
}
