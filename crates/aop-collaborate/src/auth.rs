//! Bearer tokens, checked by asking the issuer.
//!
//! This service is a resource server. It does not hold passwords, does not
//! mint tokens, and does not know how the Alterion identity provider decides
//! who somebody is. It takes the token it was handed and asks:
//!
//! ```text
//!   POST {introspection_endpoint}   token=...   ->  { active, sub, scope }
//! ```
//!
//! and the endpoint is read from `{issuer}/.well-known/openid-configuration`
//! rather than written down here, so pointing this at a self-hosted IdP is
//! one setting and not a patch.
//!
//! Every request would otherwise cost a round trip to the IdP, so an active
//! answer is held for a minute. An inactive one is never held: caching a
//! refusal would mean a token that was just issued keeps failing, and worse,
//! the code that caches "no" is one edit away from caching "yes" for a
//! revoked token.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use actix_web::{FromRequest, HttpRequest, dev::Payload, web};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::SyncError;
use crate::state::AppState;

/// A token the issuer said is real, reduced to what this service acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The account, and the only identity this server ever stores.
    pub subject: String,
    pub scope: String,
}

/// The introspection answer, as RFC 7662 defines it and the Alterion IdP
/// returns it. Everything but `active` is absent when the token is not.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Introspection {
    pub active: bool,
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

impl Introspection {
    /// The subject, but only from an answer that carries one. An `active`
    /// token with no `sub` is a client credentials token: real, but nobody,
    /// and nobody cannot own a project.
    pub fn verified(&self) -> Option<Verified> {
        if !self.active {
            return None;
        }
        let subject = self.sub.clone().filter(|s| !s.is_empty())?;
        Some(Verified { subject, scope: self.scope.clone().unwrap_or_default() })
    }
}

/// The parts of the discovery document this server uses.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub introspection_endpoint: String,
}

/// Where the discovery document lives for a given issuer.
pub fn discovery_url(issuer: &str) -> String {
    format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'))
}

/// Recently verified tokens, keyed by hash.
///
/// The key is a digest rather than the token itself so a heap dump or a
/// stray `Debug` of the map does not hand out live credentials.
pub struct TokenCache {
    ttl: Duration,
    entries: Mutex<HashMap<[u8; 32], (Verified, Instant)>>,
}

fn key_of(token: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(&Sha256::digest(token.as_bytes()));
    key
}

impl TokenCache {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl, entries: Mutex::new(HashMap::new()) }
    }

    pub fn get(&self, token: &str) -> Option<Verified> {
        self.get_at(token, Instant::now())
    }

    /// The clock is a parameter so expiry can be tested without sleeping.
    pub fn get_at(&self, token: &str, now: Instant) -> Option<Verified> {
        // The cache holds no invariant a panicking thread could have broken,
        // so recovering a poisoned map beats refusing every request after
        // one unrelated panic.
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let (who, stored_at) = entries.get(&key_of(token))?;
        (now.duration_since(*stored_at) < self.ttl).then(|| who.clone())
    }

    pub fn remember(&self, token: &str, answer: &Introspection) {
        self.remember_at(token, answer, Instant::now());
    }

    /// Store an answer, if it is one worth storing.
    ///
    /// Returning early on an inactive token is the whole point of this
    /// function existing rather than an `insert` at the call site.
    pub fn remember_at(&self, token: &str, answer: &Introspection, now: Instant) {
        let Some(who) = answer.verified() else {
            return;
        };
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        // Nothing else ever removes an entry, and a busy server sees a lot of
        // tokens, so the sweep rides along with the write.
        entries.retain(|_, (_, stored_at)| now.duration_since(*stored_at) < self.ttl);
        entries.insert(key_of(token), (who, now));
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The client this server talks to the identity provider with.
pub struct IdpClient {
    http: reqwest::Client,
    issuer: String,
    client_id: Option<String>,
    client_secret: Option<String>,
    /// Fetched once and kept: the endpoints an issuer advertises do not move
    /// between requests, and a discovery fetch per introspection would double
    /// the traffic the cache exists to avoid.
    discovery: RwLock<Option<Discovery>>,
    cache: TokenCache,
}

impl IdpClient {
    pub fn new(config: &crate::Config) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("aop-collaborate/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            issuer: config.issuer.trim_end_matches('/').to_string(),
            client_id: config.idp_client_id.clone(),
            client_secret: config.idp_client_secret.clone(),
            discovery: RwLock::new(None),
            cache: TokenCache::new(config.token_cache_ttl),
        })
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn cache(&self) -> &TokenCache {
        &self.cache
    }

    /// The introspection endpoint, from the issuer's own discovery document.
    ///
    /// There is no hardcoded fallback path on purpose. A wrong guess that
    /// happens to 404 is a confusing failure at every request; a discovery
    /// fetch that fails says which URL it tried, which is the setting the
    /// self-hoster got wrong.
    pub async fn introspection_endpoint(&self) -> Result<String, SyncError> {
        if let Some(known) = self
            .discovery
            .read()
            .ok()
            .and_then(|held| held.as_ref().map(|d| d.introspection_endpoint.clone()))
        {
            return Ok(known);
        }

        let url = discovery_url(&self.issuer);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SyncError::Idp(format!("fetch {url}: {e}")))?;
        if !response.status().is_success() {
            return Err(SyncError::Idp(format!("{url} answered {}", response.status())));
        }
        let discovery: Discovery = response
            .json()
            .await
            .map_err(|e| SyncError::Idp(format!("parse {url}: {e}")))?;

        // An issuer that does not match the one configured means a redirect
        // landed somewhere else, and trusting its endpoints would send bearer
        // tokens to a host nobody chose.
        if discovery.issuer.trim_end_matches('/') != self.issuer {
            return Err(SyncError::Idp(format!(
                "{url} claims issuer {}, expected {}",
                discovery.issuer, self.issuer
            )));
        }

        let endpoint = discovery.introspection_endpoint.clone();
        if let Ok(mut held) = self.discovery.write() {
            *held = Some(discovery);
        }
        Ok(endpoint)
    }

    /// Who this token belongs to, from cache if it was checked recently.
    pub async fn verify(&self, token: &str) -> Result<Verified, SyncError> {
        if let Some(known) = self.cache.get(token) {
            return Ok(known);
        }

        let endpoint = self.introspection_endpoint().await?;
        let mut form = vec![("token", token.to_string())];
        // Sent as form fields rather than Basic, matching the IdP's
        // client_secret_post support, and only when configured: an IdP that
        // does not require client authentication on introspection rejects
        // credentials it did not ask for.
        if let (Some(id), Some(secret)) = (&self.client_id, &self.client_secret) {
            form.push(("client_id", id.clone()));
            form.push(("client_secret", secret.clone()));
        }

        let response = self
            .http
            .post(&endpoint)
            .form(&form)
            .send()
            .await
            .map_err(|e| SyncError::Idp(format!("introspect: {e}")))?;
        if !response.status().is_success() {
            return Err(SyncError::Idp(format!(
                "introspection answered {}",
                response.status()
            )));
        }
        let answer: Introspection = response
            .json()
            .await
            .map_err(|e| SyncError::Idp(format!("parse introspection: {e}")))?;

        self.cache.remember(token, &answer);
        answer.verified().ok_or(SyncError::Unauthenticated)
    }
}

/// The bearer token from the `Authorization` header.
pub fn bearer(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

/// The same, but also accepting `?access_token=`.
///
/// Only the websocket route uses this. A browser cannot set headers on a
/// websocket handshake, so there is nowhere else to put the token, and the
/// cost is that the token can end up in an access log. That trade is worth
/// making once, for one route, and not for the REST endpoints that have a
/// header available.
pub fn bearer_or_query(req: &HttpRequest) -> Option<String> {
    bearer(req).or_else(|| {
        req.query_string()
            .split('&')
            .find_map(|pair| pair.strip_prefix("access_token="))
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
    })
}

/// Handler argument that means "an authenticated subject", so no handler can
/// forget to check.
#[derive(Debug, Clone)]
pub struct Authenticated {
    pub subject: String,
    pub scope: String,
}

impl FromRequest for Authenticated {
    type Error = SyncError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let state = req.app_data::<web::Data<AppState>>().cloned();
        let token = bearer(req);
        Box::pin(async move {
            let state = state.ok_or_else(|| SyncError::internal("state not mounted"))?;
            let token = token.ok_or(SyncError::Unauthenticated)?;
            let who = state.idp.verify(&token).await?;
            Ok(Self { subject: who.subject, scope: who.scope })
        })
    }
}
