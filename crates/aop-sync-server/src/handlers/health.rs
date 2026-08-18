//! Liveness, for whatever is watching the service.

use actix_web::{HttpResponse, get, web};
use serde_json::json;

use crate::state::AppState;

/// Unauthenticated on purpose: a health check that needs a token cannot run
/// before the identity provider is up, and then one outage looks like two.
///
/// It does touch the database, because a process that is running and cannot
/// reach Postgres is not healthy in any sense a load balancer cares about.
#[get("/api/health")]
pub async fn health(state: web::Data<AppState>) -> HttpResponse {
    let database = state.db.ping().await.is_ok();
    let body = json!({
        "status": if database { "ok" } else { "degraded" },
        "service": "aop-sync-server",
        "version": env!("CARGO_PKG_VERSION"),
        "database": database,
        "issuer": state.idp.issuer(),
    });
    if database {
        HttpResponse::Ok().json(body)
    } else {
        HttpResponse::ServiceUnavailable().json(body)
    }
}
