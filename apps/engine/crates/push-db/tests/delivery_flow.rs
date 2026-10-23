//! The accept → claim → record path against a real database (spec section 6), and
//! the invariants it owns (spec section 15). Needs `DATABASE_URL`; `sqlx::test` creates
//! a fresh database per test and applies the migrations.

use std::time::Duration;

use push_core::delivery::Reason;
use push_core::ids::{DeliveryId, EnvironmentId, InstallationId, NotificationId};
use push_core::model::{ExternalUserId, Kind, Priority, Provider, Target};
use push_core::{
    Decision, DeliveryView, FaultScope, ProviderError, RetryPolicy, SubmissionOutcome, Terminal, Timestamp,
    TransportFailure, decide,
};
use push_db::{AcceptStart, AttemptRef, Claim, ClaimedDelivery, Db, Idempotency, NewNotification, Recorded};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const LEASE: Duration = Duration::from_secs(30);

struct Fixture {
    env: EnvironmentId,
    app: Uuid,
}

async fn fixture(pool: &PgPool) -> Fixture {
    let (org, prj, env, app) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    for sql in [
        "insert into organizations (id, name) values ($1, 'Acme')",
        "insert into projects (id, organization_id, name) values ($2, $1, 'FitTrack')",
        "insert into environments (id, project_id, kind, dek_wrapped, identity_secret_ct)
           values ($3, $2, 'production', '\\x00', '\\x00')",
        "insert into apps (id, project_id, platform, bundle_id) values ($4, $2, 'ios', 'com.acme.fit')",
        "insert into app_configs (environment_id, app_id) values ($3, $4)",
    ] {
        sqlx::query(sql)
            .bind(org)
            .bind(prj)
            .bind(env)
            .bind(app)
            .execute(pool)
            .await
            .expect(sql);
    }
    Fixture {
        env: EnvironmentId::from_uuid(env),
        app,
    }
}

/// An installation with an active APNs token, optionally bound to a user.
async fn device(pool: &PgPool, f: &Fixture, user: Option<&str>) -> (InstallationId, Uuid) {
    let (ins, reg) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("insert into installations (id, environment_id, app_id, token_hash) values ($1, $2, $3, $4)")
        .bind(ins)
        .bind(f.env.as_uuid())
        .bind(f.app)
        .bind(ins.as_bytes().to_vec())
        .execute(pool)
        .await
        .expect("installation");
    sqlx::query(
        "insert into push_registrations (id, installation_id, environment_id, app_id, provider, apns_env,
                                         token_ct, token_digest)
         values ($1, $2, $3, $4, 'apns', 'production', '\\xaa', $5)",
    )
    .bind(reg)
    .bind(ins)
    .bind(f.env.as_uuid())
    .bind(f.app)
    .bind(reg.as_bytes().to_vec())
    .execute(pool)
    .await
    .expect("registration");
    if let Some(user) = user {
        sqlx::query(
            "insert into user_bindings (id, installation_id, environment_id, external_user_id, method)
             values ($1, $2, $3, $4, 'identity_token')",
        )
        .bind(Uuid::now_v7())
        .bind(ins)
        .bind(f.env.as_uuid())
        .bind(user)
        .execute(pool)
        .await
        .expect("binding");
    }
    (InstallationId::from_uuid(ins), reg)
}

fn far_future() -> Timestamp {
    Timestamp::from_unix_millis(4_102_444_800_000) // 2100-01-01
}

fn notification(target: Target) -> NewNotification {
    NewNotification {
        target,
        kind: Kind::Alert,
        priority: Priority::High,
        collapse_key: None,
        expires_at: far_future(),
        template_version_id: None,
        is_test: false,
        content_ct: b"ciphertext".to_vec(),
        purge_after: far_future(),
    }
}

fn user(id: &str) -> Target {
    Target::User(ExternalUserId::new(id).expect("valid user id"))
}

async fn send(db: &Db, f: &Fixture, target: Target, key: Option<&str>) -> (NotificationId, Vec<DeliveryId>) {
    let idem = key.map(|k| Idempotency {
        key: k.into(),
        request_hash: vec![1],
    });
    let AcceptStart::Fresh(mut tx) = db.begin_accept(f.env, idem.as_ref()).await.expect("begin") else {
        panic!("expected a fresh accept");
    };
    let recipients = tx.resolve(&target).await.expect("resolve");
    let accepted = tx.commit(&notification(target), &recipients).await.expect("commit");
    (
        accepted.notification_id,
        accepted.deliveries.into_iter().map(|(d, _)| d).collect(),
    )
}

async fn claim_send(db: &Db, n: NotificationId, d: DeliveryId) -> ClaimedDelivery {
    match db.claim(n, d, "w1", LEASE).await.expect("claim") {
        Claim::Send(c) => *c,
        other => panic!("expected Send, got {other:?}"),
    }
}

fn attempt_ref(c: &ClaimedDelivery) -> AttemptRef {
    AttemptRef {
        notification_id: c.notification_id,
        delivery_id: c.delivery_id,
        attempt_no: c.attempt_no,
        lease_epoch: c.lease_epoch,
        registration_id: c.registration_id,
        credential_id: c.credential_id,
    }
}

async fn record(db: &Db, c: &ClaimedDelivery, outcome: SubmissionOutcome) -> Recorded {
    let view = DeliveryView {
        budget_used: c.budget_used,
        ambiguous_used: c.ambiguous_used,
        expires_at: c.expires_at,
    };
    let now = Timestamp::from_unix_millis(time::OffsetDateTime::now_utc().unix_timestamp() * 1000);
    let (decision, effects) = decide(&view, &outcome, now, &RetryPolicy::default(), 0.5);
    db.record(&attempt_ref(c), &outcome, Some(1_000), false, &decision, &effects)
        .await
        .expect("record")
}

async fn notification_status(pool: &PgPool, n: NotificationId) -> (String, i16, Option<String>) {
    let r = sqlx::query("select status, deliveries_open, outcome from notifications where id = $1")
        .bind(n.as_uuid())
        .fetch_one(pool)
        .await
        .expect("notification");
    (r.get("status"), r.get("deliveries_open"), r.get("outcome"))
}

async fn delivery_state(pool: &PgPool, d: DeliveryId) -> (String, Option<String>) {
    let r = sqlx::query("select state, reason from deliveries where id = $1")
        .bind(d.as_uuid())
        .fetch_one(pool)
        .await
        .expect("delivery");
    (r.get("state"), r.get("reason"))
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.expect(sql)
}

#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn accept_fans_out_to_every_bound_device(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    device(&pool, &f, Some("user_123")).await;
    device(&pool, &f, Some("someone_else")).await;

    let (n, deliveries) = send(&db, &f, user("user_123"), None).await;
    assert_eq!(deliveries.len(), 2);
    assert_eq!(notification_status(&pool, n).await, ("processing".into(), 2, None));
}

#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn accept_with_no_devices_completes_immediately(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    let (n, deliveries) = send(&db, &f, user("nobody"), None).await;
    assert!(deliveries.is_empty());
    assert_eq!(
        notification_status(&pool, n).await,
        ("completed".into(), 0, Some("no_eligible_devices".into()))
    );
}

/// I2: the same key replays the original; a different body under the key conflicts.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn idempotency_key_replays_or_conflicts(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, _) = send(&db, &f, user("user_123"), Some("workout-1")).await;

    let same = Idempotency {
        key: "workout-1".into(),
        request_hash: vec![1],
    };
    match db.begin_accept(f.env, Some(&same)).await.expect("begin") {
        AcceptStart::Replayed(original) => assert_eq!(original, n),
        _ => panic!("expected replay"),
    }
    let different = Idempotency {
        key: "workout-1".into(),
        request_hash: vec![2],
    };
    assert!(matches!(
        db.begin_accept(f.env, Some(&different)).await.expect("begin"),
        AcceptStart::KeyConflict
    ));
    assert_eq!(count(&pool, "select count(*) from notifications").await, 1);
}

#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn accepted_send_completes_the_notification(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;

    let c = claim_send(&db, n, d[0]).await;
    assert_eq!(c.attempt_no, 1);
    // A duplicate hint finds nothing to claim.
    assert!(matches!(
        db.claim(n, d[0], "w2", LEASE).await.expect("claim"),
        Claim::Stale
    ));

    let outcome = SubmissionOutcome::Accepted {
        provider_message_id: Some("apns-1".into()),
    };
    assert_eq!(record(&db, &c, outcome).await, Recorded::Applied);
    assert_eq!(delivery_state(&pool, d[0]).await, ("provider_accepted".into(), None));
    assert_eq!(notification_status(&pool, n).await.0, "completed");
    assert_eq!(count(&pool, "select count(*) from usage_ledger").await, 1);
}

/// I10: a user who signs out before the claim does not get the push, and is not billed.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn signed_out_installation_is_suppressed(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;

    sqlx::query("update user_bindings set unbound_at = now(), unbound_reason = 'logout'")
        .execute(&pool)
        .await
        .expect("logout");

    let claim = db.claim(n, d[0], "w1", LEASE).await.expect("claim");
    assert!(matches!(
        claim,
        Claim::Finished {
            reason: Reason::RecipientUnbound
        }
    ));
    assert_eq!(
        delivery_state(&pool, d[0]).await,
        ("suppressed".into(), Some("recipient_unbound".into()))
    );
    assert_eq!(notification_status(&pool, n).await.0, "completed");
    assert_eq!(count(&pool, "select count(*) from usage_ledger").await, 0);
    assert_eq!(count(&pool, "select count(*) from delivery_attempts").await, 0);
}

/// I6: a worker whose lease was superseded cannot overwrite the new owner's result.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn stale_lease_is_fenced(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;
    let mut c = claim_send(&db, n, d[0]).await;

    c.lease_epoch -= 1; // as if another worker had re-claimed it since
    let outcome = SubmissionOutcome::Accepted {
        provider_message_id: None,
    };
    assert_eq!(record(&db, &c, outcome).await, Recorded::Fenced);
    assert_eq!(delivery_state(&pool, d[0]).await.0, "claimed");
    // The attempt itself is still recorded truthfully.
    assert_eq!(
        count(&pool, "select count(*) from delivery_attempts where state = 'accepted'").await,
        1
    );
}

/// I11: a dead-token response invalidates only the token that was used. A token that
/// rotated in while the send was in flight stays active.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn invalidation_only_hits_the_used_registration(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    let (ins, old_reg) = device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;
    let c = claim_send(&db, n, d[0]).await;
    assert_eq!(c.registration_id.as_uuid(), old_reg);

    // Token rotates mid-flight.
    let new_reg = Uuid::now_v7();
    sqlx::query("update push_registrations set state = 'superseded', ended_at = now() where id = $1")
        .bind(old_reg)
        .execute(&pool)
        .await
        .expect("supersede");
    sqlx::query(
        "insert into push_registrations (id, installation_id, environment_id, app_id, provider, apns_env,
                                         token_ct, token_digest)
         values ($1, $2, $3, $4, 'apns', 'production', '\\xbb', '\\xbb')",
    )
    .bind(new_reg)
    .bind(ins.as_uuid())
    .bind(f.env.as_uuid())
    .bind(f.app)
    .execute(&pool)
    .await
    .expect("rotate");

    let outcome = SubmissionOutcome::Rejected {
        error: ProviderError {
            provider: Provider::Apns,
            http_status: 410,
            raw_reason: "Unregistered".into(),
            reason: Reason::InvalidRegistration,
        },
        scope: FaultScope::Registration,
    };
    assert_eq!(record(&db, &c, outcome).await, Recorded::Applied);
    let state: String = sqlx::query_scalar("select state from push_registrations where id = $1")
        .bind(new_reg)
        .fetch_one(&pool)
        .await
        .expect("state");
    assert_eq!(state, "active");
}

/// I13: retries never bill twice; the retried delivery uses the current token.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn retry_reclaims_without_double_billing(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;
    let c = claim_send(&db, n, d[0]).await;

    let outcome = SubmissionOutcome::NotSent {
        failure: TransportFailure::RefusedStream,
    };
    assert_eq!(record(&db, &c, outcome).await, Recorded::Applied);
    let (state, reason) = delivery_state(&pool, d[0]).await;
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("pending", Some("transport_not_sent"))
    );

    sqlx::query("update deliveries set next_attempt_at = now()")
        .execute(&pool)
        .await
        .expect("due");
    let c2 = claim_send(&db, n, d[0]).await;
    assert_eq!(c2.attempt_no, 2);
    assert_eq!(c2.budget_used, 1);
    assert!(c2.lease_epoch > c.lease_epoch);
    assert_eq!(count(&pool, "select count(*) from usage_ledger").await, 1);
}

/// I9: pending work past its deadline is expired, never sent.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn sweeper_expires_overdue_work(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;
    sqlx::query("update deliveries set expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .expect("age");

    assert!(matches!(
        db.claim(n, d[0], "w1", LEASE).await.expect("claim"),
        Claim::Finished {
            reason: Reason::Expired
        }
    ));

    let (n2, d2) = send(&db, &f, user("user_123"), None).await;
    sqlx::query("update deliveries set expires_at = now() - interval '1 second' where id = $1")
        .bind(d2[0].as_uuid())
        .execute(&pool)
        .await
        .expect("age");
    assert_eq!(db.expire_pending(100).await.expect("sweep"), 1);
    assert_eq!(delivery_state(&pool, d2[0]).await.0, "expired");
    assert_eq!(notification_status(&pool, n2).await.0, "completed");
}

#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn lost_hints_are_found_once(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;
    sqlx::query("update deliveries set next_attempt_at = now() - interval '10 seconds'")
        .execute(&pool)
        .await
        .expect("age");

    let hints = db.take_due_hints(5, 100).await.expect("sweep");
    assert_eq!(hints.len(), 1);
    assert_eq!((hints[0].notification_id, hints[0].delivery_id), (n, d[0]));
    assert!(db.take_due_hints(5, 100).await.expect("sweep").is_empty());
}

/// A worker that dies mid-send leaves an in-flight attempt; recovery treats it as
/// ambiguous and, per decision D1, retries it once.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn expired_lease_is_recovered_as_ambiguous(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;
    let (n, d) = send(&db, &f, user("user_123"), None).await;
    claim_send(&db, n, d[0]).await;
    sqlx::query("update deliveries set lease_until = now() - interval '1 second'")
        .execute(&pool)
        .await
        .expect("age");

    let leases = db.expired_leases(100).await.expect("leases");
    assert_eq!(leases.len(), 1);
    let l = leases[0];
    let outcome = SubmissionOutcome::Indeterminate {
        failure: TransportFailure::TimeoutAfterSend,
    };
    let view = DeliveryView {
        budget_used: l.budget_used,
        ambiguous_used: l.ambiguous_used,
        expires_at: l.expires_at,
    };
    let (decision, effects) = decide(
        &view,
        &outcome,
        Timestamp::from_unix_millis(0),
        &RetryPolicy::default(),
        0.5,
    );
    assert!(matches!(decision, Decision::RetryAt { ambiguous: true, .. }));
    let attempt = AttemptRef {
        notification_id: l.notification_id,
        delivery_id: l.delivery_id,
        attempt_no: l.attempt_no,
        lease_epoch: l.lease_epoch,
        registration_id: l.registration_id,
        credential_id: l.credential_id,
    };
    let recorded = db
        .record(&attempt, &outcome, None, true, &decision, &effects)
        .await
        .expect("record");
    assert_eq!(recorded, Recorded::Applied);
    assert_eq!(
        count(
            &pool,
            "select count(*) from delivery_attempts where state = 'abandoned'"
        )
        .await,
        1
    );
    assert_eq!(count(&pool, "select ambiguous_count::bigint from deliveries").await, 1);
    assert!(!matches!(decision, Decision::Finish(Terminal::Unknown { .. })));
}

/// I2 under concurrency: 20 simultaneous requests with one key make one notification;
/// the rest wait on the first transaction and then replay it.
#[sqlx::test(migrator = "push_db::MIGRATOR")]
async fn concurrent_requests_with_one_key_make_one_notification(pool: PgPool) {
    let db = Db::new(pool.clone());
    let f = fixture(&pool).await;
    device(&pool, &f, Some("user_123")).await;

    let tasks: Vec<_> = (0..20)
        .map(|_| {
            let db = db.clone();
            let env = f.env;
            tokio::spawn(async move {
                let idem = Idempotency {
                    key: "same-key".into(),
                    request_hash: vec![7],
                };
                match db.begin_accept(env, Some(&idem)).await.expect("begin") {
                    AcceptStart::Fresh(mut tx) => {
                        let target = user("user_123");
                        let r = tx.resolve(&target).await.expect("resolve");
                        Some(
                            tx.commit(&notification(target), &r)
                                .await
                                .expect("commit")
                                .notification_id,
                        )
                    }
                    AcceptStart::Replayed(n) => Some(n),
                    AcceptStart::KeyConflict => None,
                }
            })
        })
        .collect();

    let mut ids = Vec::new();
    for t in tasks {
        ids.push(t.await.expect("task").expect("never a conflict for the same body"));
    }
    ids.dedup();
    assert_eq!(ids.len(), 1, "every request must see the same notification");
    assert_eq!(count(&pool, "select count(*) from notifications").await, 1);
    assert_eq!(count(&pool, "select count(*) from deliveries").await, 1);
}
