//! Liveness, for whatever is watching the service.

use actix_web::{HttpResponse, get, web};
use serde_json::json;

use crate::state::AppState;

/// Unauthenticated on purpose: a health check that needs a token cannot run
/// before the identity provider is up, and then one outage looks like two.
///
/// It does touch the database, because a process that is running and cannot
/// reach Postgres is not healthy in any sense a load balancer cares about.
///
/// It also says which messages this build understands. A version string
/// cannot answer that: a client holding one has to know which versions of
/// somebody else's self-hosted server gained which message, and getting that
/// wrong is how a pair looks healthy and silently does nothing. A name a
/// client can look for costs one field and answers it outright.
#[get("/api/health")]
pub async fn health(state: web::Data<AppState>) -> HttpResponse {
    let database = state.db.ping().await.is_ok();
    let body = json!({
        "status": if database { "ok" } else { "degraded" },
        "service": "aop-collaborate",
        "version": env!("CARGO_PKG_VERSION"),
        "database": database,
        "issuer": state.idp.issuer(),
        "capabilities": crate::live::CAPABILITIES,
    });
    if database {
        HttpResponse::Ok().json(body)
    } else {
        HttpResponse::ServiceUnavailable().json(body)
    }
}
