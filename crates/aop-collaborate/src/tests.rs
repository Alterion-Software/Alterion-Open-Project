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

// ── What this build says it speaks ───────────────────────────────────────────

#[test]
fn the_streaming_message_is_named_in_what_this_server_publishes() {
    // A client asks the health endpoint for this name before it offers work
    // over a socket. Dropping it here, while the message is still answered,
    // would have every up to date client quietly fall back to the REST sync.
    assert!(crate::live::CAPABILITIES.contains(&crate::live::LIVE_CHANGES));
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
fn the_shipped_default_config_parses_and_names_no_identity_provider() {
    let mut ini = configparser::ini::Ini::new();
    ini.read(crate::config::DEFAULT_CONFIG.to_string())
        .expect("the config this crate writes on first run must parse");
    let config = crate::Config::from_ini(&ini);

    // Deliberately empty. This server sends bearer tokens to the issuer for
    // introspection, so a default issuer is a default recipient for other
    // people's tokens.
    assert!(config.issuer.is_empty(), "the shipped config must name no issuer");
    assert_eq!(config.token_cache_ttl, Duration::from_secs(60));
    assert_eq!(config.bind_address, "127.0.0.1:8090");
    assert!(!config.allowed_origins.is_empty());
}

#[test]
fn an_empty_config_parses_but_will_not_start() {
    // A container that passes everything by environment ships no file, and
    // parsing that must not fail. Starting on it must.
    let config = crate::Config::from_ini(&configparser::ini::Ini::new());
    assert!(config.allowed_origins.is_empty(), "no origins means no browser origins");
    assert_eq!(config.snapshot_every, 500);
    assert!(config.validate().is_err(), "no issuer means no start");
}

#[test]
fn a_plain_http_issuer_is_refused_unless_it_is_loopback() {
    let with_issuer = |issuer: &str| {
        let mut ini = configparser::ini::Ini::new();
        ini.set("idp", "issuer", Some(issuer.to_string()));
        crate::Config::from_ini(&ini)
    };

    assert!(with_issuer("https://idp.example.test").validate().is_ok());

    // The token would be readable by anything between here and there.
    assert!(with_issuer("http://idp.example.test").validate().is_err());

    // Loopback never leaves the machine, so it is how you test without certs.
    assert!(with_issuer("http://127.0.0.1:8080").validate().is_ok());
    assert!(with_issuer("http://localhost:8080").validate().is_ok());
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

    /// The storage half of claiming, against a real database.
    ///
    /// The identity provider is not needed for this and is not stood up: what
    /// it would have said arrives as a [`Presented`], which is the reason
    /// `claim_in` takes one rather than a token. What is being checked here is
    /// the part the pure tests cannot see, which is that the membership and
    /// the deletion of the invitation are one transaction and that nothing is
    /// left to replay afterwards.
    #[actix_web::test]
    #[ignore = "needs live Postgres (set AOP_COLLAB_TEST_DATABASE_URL)"]
    async fn an_invitation_admits_its_owner_once_and_then_is_gone() {
        use chrono::Utc;
        use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};

        use crate::entity::{project_invites, project_members, projects, role};
        use crate::handlers::members::claim_in;
        use crate::sharing::{Claim, Presented};

        let db = connect().await;
        crate::schema::Migrator::up(&db, None).await.expect("up");

        let id = uuid::Uuid::new_v4();
        let now = Utc::now().fixed_offset();
        projects::ActiveModel {
            id: Set(id),
            name: Set("Bridge".into()),
            owner_subject: Set("owner-sub".into()),
            head_seq: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("a plan to share");
        project_invites::ActiveModel {
            project_id: Set(id),
            email: Set("ada@example.com".into()),
            role: Set(role::EDITOR.into()),
            invited_by: Set("owner-sub".into()),
            invited_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("an invitation");

        let wrong = claim_in(
            &db,
            id,
            "mallory-sub",
            &Presented::Verified("mallory@example.com".into()),
        )
        .await
        .expect("the call itself works");
        assert_eq!(wrong, Claim::NoInvite, "somebody else's invitation admits nobody");
        assert!(
            project_members::Entity::find_by_id((id, "mallory-sub".to_string()))
                .one(&db)
                .await
                .expect("read back")
                .is_none(),
            "a refused claim must write nothing",
        );

        let granted = claim_in(&db, id, "ada-sub", &Presented::Verified("ada@example.com".into()))
            .await
            .expect("the call itself works");
        assert_eq!(
            granted,
            Claim::Grant { role: role::EDITOR.into(), email: "ada@example.com".into() },
        );
        let member = project_members::Entity::find_by_id((id, "ada-sub".to_string()))
            .one(&db)
            .await
            .expect("read back")
            .expect("she is in");
        assert_eq!(member.role, role::EDITOR);
        assert_eq!(member.email.as_deref(), Some("ada@example.com"));
        assert!(
            project_invites::Entity::find_by_id((id, "ada@example.com".to_string()))
                .one(&db)
                .await
                .expect("read back")
                .is_none(),
            "the invitation is consumed by the same transaction that admits her",
        );

        // Removed from the plan, and then trying the invitation again. There
        // is nothing left to replay, so she stays out until somebody invites
        // her afresh.
        project_members::Entity::delete_by_id((id, "ada-sub".to_string()))
            .exec(&db)
            .await
            .expect("removed");
        let replayed = claim_in(&db, id, "ada-sub", &Presented::Verified("ada@example.com".into()))
            .await
            .expect("the call itself works");
        assert_eq!(replayed, Claim::NoInvite, "an invitation is used once");

        projects::Entity::delete_by_id(id).exec(&db).await.expect("tidy up");
    }
}

// ── Pointer presence ─────────────────────────────────────────────────────────

#[test]
fn a_client_that_sends_no_pointer_is_still_a_peer() {
    // The field was added after the first clients shipped. An older client
    // sends `{"type":"presence","row":4}` and must keep working, with no
    // pointer drawn for it rather than a refused message.
    let msg: crate::live::ClientMessage =
        serde_json::from_str(r#"{"type":"presence","row":4}"#).expect("older client must parse");
    match msg {
        crate::live::ClientMessage::Presence { row, at, .. } => {
            assert_eq!(row, Some(4));
            assert!(at.is_none(), "no pointer means none, not a default position");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn a_pointer_survives_the_round_trip_in_plan_coordinates() {
    let table: crate::live::ClientMessage = serde_json::from_str(
        r#"{"type":"presence","row":2,"at":{"pane":"table","row":2,"column":5}}"#,
    )
    .expect("table pointer");
    let chart: crate::live::ClientMessage = serde_json::from_str(
        r#"{"type":"presence","at":{"pane":"chart","row":7,"minutes":960}}"#,
    )
    .expect("chart pointer");

    match table {
        crate::live::ClientMessage::Presence {
            at: Some(crate::live::Pointer::Table { row, column }), ..
        } => {
            assert_eq!((row, column), (2, 5));
        }
        _ => panic!("expected a table pointer"),
    }
    match chart {
        crate::live::ClientMessage::Presence {
            row, at: Some(crate::live::Pointer::Chart { row: r, minutes }), ..
        } => {
            // A pointer with no selection is normal: moving the mouse is not
            // selecting anything.
            assert!(row.is_none());
            assert_eq!((r, minutes), (7, 960));
        }
        _ => panic!("expected a chart pointer"),
    }
}

#[test]
fn presence_without_a_pointer_does_not_serialise_the_field() {
    // Pointer moves are frequent, so an absent pointer must not cost bytes on
    // every message that does not carry one.
    let json = serde_json::to_string(&crate::live::Presence {
        subject: "s".into(),
        name: "Ada".into(),
        row: Some(1),
        at: None,
        picture: None,
        editing: None,
        draft: None,
    })
    .expect("serialises");
    assert!(!json.contains("\"at\""), "absent pointer must be omitted, got {json}");
}

#[test]
fn an_introspection_scope_is_read_as_a_string_or_a_list() {
    // RFC 7662 defines one space separated string. A provider that keeps
    // scopes in a JSON column sends an array, which is the same information,
    // so refusing it would fail closed over nothing. This exact mismatch made
    // every real token 502 while bogus ones correctly returned 401, because
    // an inactive answer carries no scope to disagree about.
    let as_string: crate::auth::Introspection =
        serde_json::from_str(r#"{"active":true,"sub":"u1","scope":"openid profile"}"#).expect("string form");
    let as_list: crate::auth::Introspection =
        serde_json::from_str(r#"{"active":true,"sub":"u1","scope":["openid","profile"]}"#).expect("list form");
    let absent: crate::auth::Introspection =
        serde_json::from_str(r#"{"active":false}"#).expect("inactive");

    assert_eq!(as_string.scope.as_deref(), Some("openid profile"));
    assert_eq!(as_list.scope.as_deref(), Some("openid profile"));
    assert!(absent.scope.is_none());
    assert!(as_list.verified().is_some(), "a real token must verify");
    assert!(absent.verified().is_none());
}

// ── Sharing: who gets let in ─────────────────────────────────────────────────

mod sharing {
    use actix_web::ResponseError;

    use crate::entity::role;
    use crate::error::SyncError;
    use crate::sharing::{Claim, Offered, Presented, address, decide, manager, presented, removable};

    fn invited(email: &str, role: &str) -> Offered {
        Offered { email: email.into(), role: role.into() }
    }

    fn verified(email: &str) -> Presented {
        Presented::Verified(email.into())
    }

    #[test]
    fn an_invitation_is_claimed_by_the_address_it_names() {
        let invite = invited("ada@example.com", role::EDITOR);
        assert_eq!(
            decide(None, &verified("ada@example.com"), Some(&invite)),
            Claim::Grant { role: role::EDITOR.into(), email: "ada@example.com".into() },
        );
    }

    #[test]
    fn somebody_elses_invitation_is_not_claimed_by_whoever_asks() {
        // The lookup is by address, so this can only disagree if the lookup is
        // ever written wrongly. That is exactly the day it matters: the thing
        // being prevented is one person's invitation admitting another.
        let invite = invited("ada@example.com", role::EDITOR);
        assert_eq!(
            decide(None, &verified("mallory@example.com"), Some(&invite)),
            Claim::NoInvite,
        );
    }

    #[test]
    fn capitalisation_and_stray_spaces_do_not_lose_an_invitation() {
        // The owner types it and the provider says it, and neither agrees with
        // the other about capitals. An invitation that silently never matches
        // is a feature that appears to work.
        let invite = invited(
            &address("  Ada@Example.COM ").expect("an address with room around it"),
            role::VIEWER,
        );
        assert_eq!(invite.email, "ada@example.com");
        assert!(matches!(
            decide(None, &verified("ada@example.com"), Some(&invite)),
            Claim::Grant { .. }
        ));
    }

    #[test]
    fn a_consumed_invitation_cannot_be_used_a_second_time() {
        // Claiming deletes the row inside the transaction that writes the
        // membership, so the replay arrives with nothing to find. This is what
        // stops somebody who has been removed from a plan walking back in
        // through the invitation they came by.
        assert_eq!(decide(None, &verified("ada@example.com"), None), Claim::NoInvite);
    }

    #[test]
    fn an_unverified_address_is_refused_and_says_why() {
        // Without this, anybody with an account claims any invitation by
        // writing the invited address into their own profile, and the whole
        // scheme is decoration.
        let invite = invited("ada@example.com", role::EDITOR);
        let unverified = presented(Some("ada@example.com"), Some(false));
        let unsaid = presented(Some("ada@example.com"), None);

        for answer in [unverified, unsaid] {
            match decide(None, &answer, Some(&invite)) {
                Claim::NotVerified(why) => assert!(
                    why.contains("confirm") || why.contains("Confirm"),
                    "nothing to do about it: {why}"
                ),
                other => panic!("an unconfirmed address must not be admitted, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_provider_that_names_no_address_admits_nobody() {
        let invite = invited("ada@example.com", role::EDITOR);
        assert!(matches!(
            decide(None, &presented(None, Some(true)), Some(&invite)),
            Claim::NotVerified(_)
        ));
    }

    #[test]
    fn a_provider_that_cannot_be_reached_fails_closed_and_says_so() {
        // The distinction the whole endpoint turns on: "nobody could check"
        // must not be answered as "you were not invited". One of those is a
        // reason to try again and the other is not.
        let invite = invited("ada@example.com", role::EDITOR);
        let out = Presented::Unavailable("userinfo: connection refused".into());

        let refusal = decide(None, &out, Some(&invite));
        assert_eq!(refusal, Claim::CannotCheck("userinfo: connection refused".into()));
        assert_ne!(refusal, Claim::NoInvite, "an outage is not a refusal");
        // 502 rather than 404: the caller is told the server could not ask,
        // which is true, rather than that there is nothing for them.
        assert_eq!(SyncError::Idp(String::new()).status_code(), 502);
    }

    #[test]
    fn somebody_already_in_is_left_exactly_as_they_were() {
        // There is no path in this server that changes a role once it is held,
        // which is what stops a stale invitation quietly demoting an editor to
        // a viewer the next time they open the plan.
        let stale = invited("ada@example.com", role::VIEWER);
        assert_eq!(
            decide(Some(role::EDITOR), &verified("ada@example.com"), Some(&stale)),
            Claim::Already(role::EDITOR.into()),
        );
    }

    #[test]
    fn an_invitation_carrying_a_role_it_could_not_have_been_given_admits_nobody() {
        // A row with owner on it did not come through the invite endpoint, so
        // it came from somebody editing the table. Refusing beats guessing
        // which access was meant.
        let hand_written = invited("ada@example.com", role::OWNER);
        assert_eq!(
            decide(None, &verified("ada@example.com"), Some(&hand_written)),
            Claim::NoInvite,
        );
    }

    #[test]
    fn a_non_member_is_told_the_plan_is_not_there_and_never_that_it_is_not_theirs() {
        // The rule the whole surface depends on. "Forbidden" confirms the id
        // is real to anybody who tries a few; "not found" tells them nothing.
        let refusal = manager(None).expect_err("a non-member manages nothing");
        assert_eq!(refusal.status_code(), 404);
        assert_ne!(refusal.status_code(), 403, "that would confirm the id is real");
    }

    #[test]
    fn a_member_who_is_not_the_owner_cannot_invite_or_remove() {
        // A real refusal this time, because a member already knows the plan is
        // real and has nothing to learn from being told so.
        for held in [role::EDITOR, role::VIEWER] {
            let refusal = manager(Some(held.into()))
                .expect_err("only the owner shares a plan out");
            assert_eq!(refusal.status_code(), 403, "{held} is a member, so it is a refusal");
        }
        assert_eq!(manager(Some(role::OWNER.into())).ok().as_deref(), Some(role::OWNER));
    }

    #[test]
    fn the_owner_cannot_be_removed_by_anybody_including_themselves() {
        // A plan whose owner has been removed is one nobody can share, nobody
        // can delete, and whose owner_subject names an account with no way in.
        assert!(removable("owner-sub", "owner-sub").is_err());
        assert!(removable("owner-sub", "ada-sub").is_ok());

        let refusal = removable("owner-sub", "owner-sub").expect_err("refused");
        // Not a 403: there is no caller for whom this would have worked, so it
        // is a request that cannot be honoured rather than one that is barred.
        assert_eq!(refusal.status_code(), 400);
        assert!(refusal.to_string().contains("Delete the plan"), "{refusal}");
    }

    #[test]
    fn an_invitation_cannot_hand_out_ownership() {
        assert_eq!(role::invitable("editor"), Some(role::EDITOR));
        assert_eq!(role::invitable(" VIEWER "), Some(role::VIEWER));
        assert_eq!(role::invitable("owner"), None, "a plan has one owner");
        assert_eq!(role::invitable("admin"), None);
    }

    #[test]
    fn only_one_address_is_ever_stored() {
        assert_eq!(address("ada@example.com").as_deref(), Some("ada@example.com"));
        assert_eq!(address("ada+plans@example.co.uk").as_deref(), Some("ada+plans@example.co.uk"));

        // A newline in a stored address is one field becoming two, and a
        // header injection waiting for the day something here sends mail.
        assert_eq!(address("ada@example.com\nBcc: mallory@example.com"), None);
        assert_eq!(address("Ada Lovelace <ada@example.com>"), None);
        assert_eq!(address("ada@example.com, bob@example.com"), None);
        assert_eq!(address("ada"), None);
        assert_eq!(address("@example.com"), None);
        assert_eq!(address("ada@"), None);
        assert_eq!(address(""), None);
        // A primary key nobody else should have to pay for.
        assert_eq!(address(&format!("{}@example.com", "a".repeat(300))), None);
    }
}

// ── The picture on a presence ────────────────────────────────────────────────

#[test]
fn a_client_that_sends_no_picture_is_still_a_peer() {
    // The field was added after the first clients shipped, so an older hello
    // has to keep working and simply have no face drawn for it.
    let older: crate::live::ClientMessage =
        serde_json::from_str(r#"{"type":"hello","after":42,"name":"Grace"}"#)
            .expect("an older hello must parse");
    match older {
        crate::live::ClientMessage::Hello { after, name, picture } => {
            assert_eq!(after, Some(42));
            assert_eq!(name.as_deref(), Some("Grace"));
            assert!(picture.is_none(), "no picture means none, not a placeholder");
        }
        _ => panic!("wrong variant"),
    }

    let newer: crate::live::ClientMessage = serde_json::from_str(
        r#"{"type":"hello","after":42,"name":"Grace","picture":"https://idp.example.test/g.png"}"#,
    )
    .expect("a newer hello must parse");
    match newer {
        crate::live::ClientMessage::Hello { picture, .. } => {
            assert_eq!(picture.as_deref(), Some("https://idp.example.test/g.png"));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn a_presence_with_no_picture_does_not_serialise_the_field() {
    // Most accounts have no picture, and presence is the most frequent
    // message on the socket: an absent one must not cost bytes.
    let json = serde_json::to_string(&crate::live::Presence {
        subject: "s".into(),
        name: "Ada".into(),
        row: Some(1),
        at: None,
        picture: None,
        editing: None,
        draft: None,
    })
    .expect("serialises");
    assert!(!json.contains("picture"), "absent picture must be omitted, got {json}");
    // The cell and the draft are the exception, and deliberately so: they are
    // stated whole every time, because a copy receiving one cannot tell an
    // absent field from a cell that has just been closed.
    assert!(json.contains("\"editing\":null"), "the cell is stated whole, got {json}");
    assert!(json.contains("\"draft\":null"), "and so is the draft, got {json}");
}

#[test]
fn a_picture_address_is_bounded_before_it_is_relayed() {
    // Whatever one client sends here is echoed to everybody in the room, so
    // its size is everybody's problem. What the URL is allowed to *be* is
    // decided by the copy that would load it, not guessed at here.
    let long = "https://idp.example.test/".to_string() + &"a".repeat(9_000);
    let kept = crate::live::picture_worth_keeping(Some(long)).expect("kept, but shorter");
    assert!(kept.len() <= 2048);
    assert_eq!(crate::live::picture_worth_keeping(Some("   ".into())), None);
    assert_eq!(crate::live::picture_worth_keeping(None), None);
}

// ── The ephemeral channel ────────────────────────────────────────────────────

#[test]
fn an_absent_cell_means_unchanged_and_a_null_one_means_closed() {
    // These are opposite instructions and plain serde collapses them into one
    // answer. A pointer move would otherwise close everybody else's view of
    // the cell somebody has open.
    let moved: crate::live::ClientMessage = serde_json::from_str(
        r#"{"type":"presence","at":{"pane":"table","row":2,"column":1}}"#,
    )
    .expect("a pointer move");
    let closed: crate::live::ClientMessage =
        serde_json::from_str(r#"{"type":"presence","editing":null,"draft":null}"#)
            .expect("an edit being abandoned");
    let opened: crate::live::ClientMessage = serde_json::from_str(
        r#"{"type":"presence","editing":{"row":4,"column":1},"draft":"Pour the "}"#,
    )
    .expect("an edit in progress");

    match moved {
        crate::live::ClientMessage::Presence { editing, draft, .. } => {
            assert!(editing.is_none(), "absent is nothing to say");
            assert!(draft.is_none());
        }
        _ => panic!("wrong variant"),
    }
    match closed {
        crate::live::ClientMessage::Presence { editing, draft, .. } => {
            assert_eq!(editing, Some(None), "null is a cell being closed");
            assert_eq!(draft, Some(None));
        }
        _ => panic!("wrong variant"),
    }
    match opened {
        crate::live::ClientMessage::Presence { editing, draft, .. } => {
            assert_eq!(editing, Some(Some(crate::live::Cell { row: 4, column: 1 })));
            assert_eq!(draft.flatten().as_deref(), Some("Pour the "));
        }
        _ => panic!("wrong variant"),
    }
}

// ── Changes over the socket ──────────────────────────────────────────────────

#[test]
fn work_offered_over_the_socket_carries_the_same_cursor_a_push_does() {
    // The socket is a second way to reach the push, not a second protocol, so
    // the message it carries is the same question: here is work made after
    // cursor N.
    let msg: crate::live::ClientMessage = serde_json::from_str(
        r#"{"type":"changes","after":42,"changes":[
            {"id":7,"at":"2026-08-18T09:00:00","author":"Grace",
             "script":"indent();","summary":"Indented a task"}]}"#,
    )
    .expect("a streamed batch");
    match msg {
        crate::live::ClientMessage::Changes { after, changes } => {
            assert_eq!(after, Some(42));
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].id, 7);
        }
        _ => panic!("wrong variant"),
    }

    // And a client that has never synced says so the same way a push does.
    let first: crate::live::ClientMessage =
        serde_json::from_str(r#"{"type":"changes","changes":[]}"#).expect("an empty first offer");
    match first {
        crate::live::ClientMessage::Changes { after, changes } => {
            assert!(after.is_none(), "no cursor is how a client says it has never synced");
            assert!(changes.is_empty());
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn the_socket_says_which_seq_each_change_was_given() {
    // The client numbers its own work and cannot know what the log called it,
    // so the answer has to name both or nothing can be marked as sent.
    let message = crate::live::ServerMessage::Applied {
        head: 45,
        applied: vec![
            crate::sync::Assigned { local_id: 7, seq: 44 },
            crate::sync::Assigned { local_id: 8, seq: 45 },
        ],
        snapshot_wanted: false,
    };
    let text = message.encode().expect("encodes");
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(value["type"], "applied");
    assert_eq!(value["head"], 45);
    assert_eq!(value["applied"][0]["local_id"], 7);
    assert_eq!(value["applied"][0]["seq"], 44);
}

#[test]
fn a_welcome_tells_the_client_which_connection_it_is() {
    // The client cannot work this out for itself, and it needs it: a REST
    // push carries it back as `connection`, and the append then skips this
    // socket when it broadcasts. Without it, a client holding a socket is sent
    // its own push straight back down it, renumbered, and applies it twice.
    let message = crate::live::ServerMessage::Welcome {
        head: 45,
        peers: Vec::new(),
        connection: 11,
    };
    let text = message.encode().expect("encodes");
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(value["type"], "welcome");
    assert_eq!(value["connection"], 11);
}

#[test]
fn a_push_that_names_no_connection_is_still_a_push() {
    // An older client sends no such field, and must go on working exactly as
    // it did: everything on the project hears about its work, itself included,
    // which is what happened before this existed.
    let body: crate::handlers::changes::Push =
        serde_json::from_str(r#"{"after":42,"changes":[]}"#).expect("the older body still reads");
    assert_eq!(body.after, Some(42));
    assert_eq!(body.connection, None);
}

#[test]
fn a_streamed_refusal_says_the_same_four_things_a_rest_refusal_does() {
    // The client already knows how to answer behind, gap and ahead, because
    // the REST push taught it. The socket must not invent new words for them.
    let behind = crate::live::ServerMessage::Behind {
        head: 45,
        after: 42,
        changes: Vec::new(),
        more: false,
    };
    let ahead = crate::live::ServerMessage::Ahead { head: 12, cursor: 60 };
    let gap = crate::live::ServerMessage::Gap { head: 45, oldest: Some(38) };

    for (message, expected) in [(behind, "behind"), (ahead, "ahead"), (gap, "gap")] {
        let text = message.encode().expect("encodes");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(value["type"], expected);
        assert!(value["head"].as_i64().is_some(), "every refusal names the head");
    }
}
