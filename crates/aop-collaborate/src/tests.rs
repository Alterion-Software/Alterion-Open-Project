//! Unit tests for the parts that decide things.
//!
//! Everything here runs without a database and without the identity provider,
//! which is the point: the push decision and the token cache are the two
//! places this server can be wrong in a way that loses somebody's work or
//! lets somebody in, and neither should need a live environment to check.
//!
//! The tests that do need Postgres are at the bottom and are ignored by
//! default, the way `alterion-auth/src/tests.rs` does it.

use std::time::{Duration, Instant};

use crate::auth::{Introspection, TokenCache, Verified, discovery_url};
use crate::sync::{LogRange, PushDecision, assign, decide, wants_snapshot};

fn range(head: i64, oldest: Option<i64>) -> LogRange {
    LogRange { head, oldest }
}

// ── The push decision ────────────────────────────────────────────────────────

#[test]
fn a_first_push_to_an_empty_log_appends_at_one() {
    // Seq counts from 1 so that a cursor of 0 can mean "I have nothing"
    // without needing an option on the wire.
    let decision = decide(LogRange::empty(), None);
    assert_eq!(decision, PushDecision::Append { first_seq: 1 });
    assert!(!decision.is_conflict());
}

#[test]
fn a_client_at_the_head_appends_after_it() {
    assert_eq!(
        decide(range(42, Some(1)), Some(42)),
        PushDecision::Append { first_seq: 43 }
    );
}

#[test]
fn a_client_that_is_behind_is_told_what_it_missed() {
    // The refusal has to carry the cursor the missed changes start after, or
    // the client cannot ask for them without guessing.
    let decision = decide(range(45, Some(1)), Some(42));
    assert_eq!(decision, PushDecision::Behind { head: 45, missed_after: 42 });
    assert!(decision.is_conflict(), "behind is a conflict, not a success");

    let body = decision.body();
    assert_eq!(body["status"], "behind");
    assert_eq!(body["head"], 45);
    assert_eq!(body["after"], 42);
    assert!(body["changes"].is_array(), "the handler fills these in");
}

#[test]
fn a_cursor_older_than_the_kept_log_is_a_gap_and_not_merely_behind() {
    // This is the distinction that matters: a client told "behind" would
    // rebase onto an answer that silently skipped the trimmed entries.
    let decision = decide(range(45, Some(38)), Some(12));
    assert_eq!(decision, PushDecision::Gap { head: 45, oldest: Some(38) });
    assert_eq!(decision.body()["status"], "gap");
}

#[test]
fn the_oldest_kept_entry_is_itself_not_a_gap() {
    // Oldest kept is 38, so a client at 37 still gets 38 onwards intact.
    // Off by one here is the difference between a working sync and every
    // client being sent a snapshot forever.
    assert!(!range(45, Some(38)).has_gap_since(37));
    assert!(range(45, Some(38)).has_gap_since(36));
}

#[test]
fn an_emptied_log_only_lets_a_client_at_the_head_through() {
    // Mirrors History::has_gap_since with nothing kept: only a cursor already
    // at the end can be trusted.
    let emptied = range(45, None);
    assert!(emptied.has_gap_since(44));
    assert!(!emptied.has_gap_since(45));
}

#[test]
fn a_cursor_past_the_head_is_refused_rather_than_appended() {
    // A restored backup, or a client pointed at the wrong instance. Appending
    // would interleave two histories that share numbers but not events.
    let decision = decide(range(12, Some(1)), Some(60));
    assert_eq!(decision, PushDecision::Ahead { head: 12, cursor: 60 });
    assert_eq!(decision.body()["status"], "ahead");
    assert!(decision.is_conflict());
}

#[test]
fn every_refusal_names_the_head_to_sync_to() {
    // One field the client can always read, whichever answer it got.
    for decision in [
        decide(range(45, Some(1)), Some(42)),
        decide(range(45, Some(38)), Some(12)),
        decide(range(12, Some(1)), Some(60)),
    ] {
        assert!(decision.is_conflict());
        assert!(decision.body()["head"].as_i64().is_some());
    }
}

// ── Cursor arithmetic ────────────────────────────────────────────────────────

#[test]
fn pushed_changes_take_consecutive_seqs_from_the_head() {
    let seqs: Vec<i64> = assign(43, 3).collect();
    assert_eq!(seqs, vec![43, 44, 45]);
}

#[test]
fn an_empty_push_takes_no_seqs() {
    assert_eq!(assign(43, 0).count(), 0, "asking if you are current writes nothing");
}

#[test]
fn a_snapshot_is_wanted_once_the_log_runs_far_enough_past_the_last_one() {
    assert!(!wants_snapshot(499, Some(0), 500));
    assert!(wants_snapshot(500, Some(0), 500));
    assert!(!wants_snapshot(600, Some(200), 500), "the newest snapshot resets the count");
    assert!(wants_snapshot(500, None, 500), "no snapshot at all counts from zero");
}

// ── The introspection cache ──────────────────────────────────────────────────

fn active(sub: &str) -> Introspection {
    Introspection {
        active: true,
        sub: Some(sub.to_string()),
        scope: Some("openid plans".into()),
    }
}

#[test]
fn an_active_token_is_remembered_for_the_ttl() {
    let cache = TokenCache::new(Duration::from_secs(60));
    let now = Instant::now();
    cache.remember_at("token-a", &active("user-1"), now);

    assert_eq!(
        cache.get_at("token-a", now + Duration::from_secs(59)),
        Some(Verified { subject: "user-1".into(), scope: "openid plans".into() }),
    );
}

#[test]
fn a_remembered_token_expires_rather_than_lingering() {
    // The ttl is the window in which a revoked token still works, so it has
    // to actually end.
    let cache = TokenCache::new(Duration::from_secs(60));
    let now = Instant::now();
    cache.remember_at("token-a", &active("user-1"), now);

    assert!(cache.get_at("token-a", now + Duration::from_secs(60)).is_none());
    assert!(cache.get_at("token-a", now + Duration::from_secs(3600)).is_none());
}

#[test]
fn an_inactive_token_is_never_cached() {
    // Caching a refusal means a token issued one second later keeps failing,
    // and the code that caches "no" is one edit from caching a stale "yes".
    let cache = TokenCache::new(Duration::from_secs(60));
    let now = Instant::now();
    let revoked = Introspection { active: false, sub: Some("user-1".into()), scope: None };

    cache.remember_at("token-a", &revoked, now);
    assert!(cache.is_empty(), "nothing about an inactive token is worth keeping");
    assert!(cache.get_at("token-a", now).is_none());
}

#[test]
fn an_active_token_with_no_subject_is_not_cached_either() {
    // A client credentials token is real and belongs to nobody, and nobody
    // cannot own a project.
    let cache = TokenCache::new(Duration::from_secs(60));
    let now = Instant::now();
    let machine = Introspection { active: true, sub: None, scope: Some("plans".into()) };

    cache.remember_at("token-a", &machine, now);
    assert!(cache.is_empty());
}

#[test]
fn two_tokens_do_not_share_an_entry() {
    let cache = TokenCache::new(Duration::from_secs(60));
    let now = Instant::now();
    cache.remember_at("token-a", &active("user-1"), now);
    cache.remember_at("token-b", &active("user-2"), now);

    assert_eq!(cache.get_at("token-a", now).map(|v| v.subject), Some("user-1".into()));
    assert_eq!(cache.get_at("token-b", now).map(|v| v.subject), Some("user-2".into()));
    assert!(cache.get_at("token-c", now).is_none());
}

#[test]
fn expired_entries_do_not_accumulate() {
    let cache = TokenCache::new(Duration::from_secs(60));
    let now = Instant::now();
    cache.remember_at("token-a", &active("user-1"), now);
    cache.remember_at("token-b", &active("user-2"), now + Duration::from_secs(120));

    assert_eq!(cache.len(), 1, "the write sweeps what the ttl has already ended");
}

// ── Discovery ────────────────────────────────────────────────────────────────

#[test]
fn discovery_hangs_off_the_issuer_however_it_was_written() {
    assert_eq!(
        discovery_url("https://auth.coraldune.cloud"),
        "https://auth.coraldune.cloud/.well-known/openid-configuration"
    );
    assert_eq!(
        discovery_url("https://idp.example.test/"),
        "https://idp.example.test/.well-known/openid-configuration",
        "a trailing slash must not produce a double slash"
    );
}

// ── Configuration ────────────────────────────────────────────────────────────

#[test]
fn the_shipped_default_config_parses_and_points_at_the_public_idp() {
    let mut ini = configparser::ini::Ini::new();
    ini.read(crate::config::DEFAULT_CONFIG.to_string())
        .expect("the config this crate writes on first run must parse");
    let config = crate::Config::from_ini(&ini);

    assert_eq!(config.issuer, "https://auth.coraldune.cloud");
    assert_eq!(config.token_cache_ttl, Duration::from_secs(60));
    assert_eq!(config.bind_address, "127.0.0.1:8090");
    assert!(!config.allowed_origins.is_empty());
}

#[test]
fn an_empty_config_still_starts_with_working_defaults() {
    // A container that passes everything by environment ships no file, and
    // that must not be a startup failure.
    let config = crate::Config::from_ini(&configparser::ini::Ini::new());
    assert_eq!(config.issuer, "https://auth.coraldune.cloud");
    assert!(config.allowed_origins.is_empty(), "no origins means no browser origins");
    assert_eq!(config.snapshot_every, 500);
}

// ── Live integration (needs Postgres; ignored by default) ────────────────────

mod live_database {
    use sea_orm::{Database, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    async fn connect() -> DatabaseConnection {
        let url = std::env::var("AOP_COLLAB_TEST_DATABASE_URL")
            .expect("set AOP_COLLAB_TEST_DATABASE_URL to run this");
        Database::connect(&url).await.expect("connect")
    }

    #[actix_web::test]
    #[ignore = "needs live Postgres (set AOP_COLLAB_TEST_DATABASE_URL)"]
    async fn migrations_apply_and_roll_back() {
        let db = connect().await;
        crate::schema::Migrator::up(&db, None).await.expect("up");
        crate::schema::Migrator::down(&db, None).await.expect("down");
    }
}
