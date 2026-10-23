-- Schema-level invariants from spec section 15, run against a freshly migrated database:
--   psql -v ON_ERROR_STOP=1 -f migrations/0001_init.sql -f tests/sql/invariants.sql
-- Every check runs inside one transaction that is rolled back at the end.

\o /dev/null
begin;

create function pg_temp.expect_violation(label text, stmt text) returns void language plpgsql as $$
begin
  execute stmt;
  raise exception 'FAIL %: statement succeeded but should have been rejected', label;
exception
  when unique_violation or check_violation or foreign_key_violation or not_null_violation then
    raise notice 'ok   %', label;
end $$;

-- Fixtures: one environment with one iOS app and one installation.
insert into organizations values ('00000000-0000-7000-8000-000000000001', 'Acme');
insert into projects values ('00000000-0000-7000-8000-000000000002', '00000000-0000-7000-8000-000000000001', 'FitTrack');
insert into environments (id, project_id, kind, dek_wrapped, identity_secret_ct)
  values ('00000000-0000-7000-8000-000000000003', '00000000-0000-7000-8000-000000000002', 'production', '\x00', '\x00');
insert into apps values ('00000000-0000-7000-8000-000000000004', '00000000-0000-7000-8000-000000000002', 'ios', 'com.acme.fit');
insert into app_configs (environment_id, app_id)
  values ('00000000-0000-7000-8000-000000000003', '00000000-0000-7000-8000-000000000004');
insert into installations (id, environment_id, app_id, token_hash)
  values ('00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000003', '00000000-0000-7000-8000-000000000004', '\x01'),
         ('00000000-0000-7000-8000-000000000011', '00000000-0000-7000-8000-000000000003', '00000000-0000-7000-8000-000000000004', '\x02');
insert into push_registrations (id, installation_id, environment_id, app_id, provider, apns_env, token_ct, token_digest)
  values ('00000000-0000-7000-8000-000000000020', '00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000003',
          '00000000-0000-7000-8000-000000000004', 'apns', 'production', '\xaa', '\xd1');

-- I4: at most one active registration per installation.
select pg_temp.expect_violation('I4 second active token on one installation', $s$
  insert into push_registrations (id, installation_id, environment_id, app_id, provider, apns_env, token_ct, token_digest)
  values ('00000000-0000-7000-8000-000000000021', '00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000003',
          '00000000-0000-7000-8000-000000000004', 'apns', 'production', '\xbb', '\xd2') $s$);

-- I4: at most one active owner per token.
select pg_temp.expect_violation('I4 same token active on two installations', $s$
  insert into push_registrations (id, installation_id, environment_id, app_id, provider, apns_env, token_ct, token_digest)
  values ('00000000-0000-7000-8000-000000000022', '00000000-0000-7000-8000-000000000011', '00000000-0000-7000-8000-000000000003',
          '00000000-0000-7000-8000-000000000004', 'apns', 'production', '\xaa', '\xd1') $s$);

-- A superseded token frees the slot (token rotation works).
update push_registrations set state = 'superseded', ended_at = now() where id = '00000000-0000-7000-8000-000000000020';
insert into push_registrations (id, installation_id, environment_id, app_id, provider, apns_env, token_ct, token_digest)
  values ('00000000-0000-7000-8000-000000000023', '00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000003',
          '00000000-0000-7000-8000-000000000004', 'apns', 'production', '\xcc', '\xd3');

select pg_temp.expect_violation('APNs registration without an APNs environment', $s$
  insert into push_registrations (id, installation_id, environment_id, app_id, provider, token_ct, token_digest)
  values ('00000000-0000-7000-8000-000000000024', '00000000-0000-7000-8000-000000000011', '00000000-0000-7000-8000-000000000003',
          '00000000-0000-7000-8000-000000000004', 'apns', '\xdd', '\xd4') $s$);

-- I5: at most one active user per installation.
insert into user_bindings (id, installation_id, environment_id, external_user_id, method)
  values ('00000000-0000-7000-8000-000000000030', '00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000003', 'user_123', 'identity_token');
select pg_temp.expect_violation('I5 second active user on one installation', $s$
  insert into user_bindings (id, installation_id, environment_id, external_user_id, method)
  values ('00000000-0000-7000-8000-000000000031', '00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000003', 'user_456', 'identity_token') $s$);
select pg_temp.expect_violation('external user id over 256 bytes', $s$
  insert into user_bindings (id, installation_id, environment_id, external_user_id, method)
  values ('00000000-0000-7000-8000-000000000032', '00000000-0000-7000-8000-000000000011', '00000000-0000-7000-8000-000000000003', repeat('x', 257), 'server_api') $s$);

-- I2: one notification per (environment, idempotency key).
insert into notifications (id, environment_id, target_kind, target_value, kind, priority, collapse_key, expires_at, status, deliveries_total, deliveries_open)
  values ('00000000-0000-7000-8000-000000000040', '00000000-0000-7000-8000-000000000003', 'user', 'user_123', 'alert', 'high', 'c', now() + interval '1 day', 'processing', 1, 1);
insert into idempotency_keys values ('00000000-0000-7000-8000-000000000003', 'workout-123', '\x01', '00000000-0000-7000-8000-000000000040', now() + interval '1 day');
select pg_temp.expect_violation('I2 idempotency key reused', $s$
  insert into idempotency_keys values ('00000000-0000-7000-8000-000000000003', 'workout-123', '\x02', '00000000-0000-7000-8000-000000000040', now() + interval '1 day') $s$);

select pg_temp.expect_violation('open deliveries above total', $s$
  update notifications set deliveries_open = 2 where id = '00000000-0000-7000-8000-000000000040' $s$);

-- I3: at most one delivery per (notification, installation).
insert into deliveries (notification_id, id, environment_id, installation_id, app_id, provider, next_attempt_at, expires_at)
  values ('00000000-0000-7000-8000-000000000040', '00000000-0000-7000-8000-000000000050', '00000000-0000-7000-8000-000000000003',
          '00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000004', 'apns', now(), now() + interval '1 day');
select pg_temp.expect_violation('I3 second delivery to one installation', $s$
  insert into deliveries (notification_id, id, environment_id, installation_id, app_id, provider, next_attempt_at, expires_at)
  values ('00000000-0000-7000-8000-000000000040', '00000000-0000-7000-8000-000000000051', '00000000-0000-7000-8000-000000000003',
          '00000000-0000-7000-8000-000000000010', '00000000-0000-7000-8000-000000000004', 'apns', now(), now() + interval '1 day') $s$);

select pg_temp.expect_violation('claimed delivery without a lease', $s$
  update deliveries set state = 'claimed' where id = '00000000-0000-7000-8000-000000000050' $s$);

-- I6: the claim is fenced by lease_epoch; a stale epoch updates nothing.
update deliveries set state = 'claimed', lease_epoch = lease_epoch + 1, lease_until = now() + interval '30 seconds',
       attempt_count = attempt_count + 1
 where notification_id = '00000000-0000-7000-8000-000000000040' and id = '00000000-0000-7000-8000-000000000050' and state = 'pending';
do $$
declare n int;
begin
  update deliveries set state = 'provider_accepted', lease_until = null, finished_at = now()
   where notification_id = '00000000-0000-7000-8000-000000000040' and id = '00000000-0000-7000-8000-000000000050' and lease_epoch = 0;
  get diagnostics n = row_count;
  if n <> 0 then raise exception 'FAIL I6: stale epoch updated a delivery'; end if;
  raise notice 'ok   I6 stale lease epoch is fenced';
end $$;

-- I7 support: at most one attempt in flight per delivery.
insert into delivery_attempts (notification_id, delivery_id, attempt_no, lease_epoch, registration_id, worker_id, started_at)
  values ('00000000-0000-7000-8000-000000000040', '00000000-0000-7000-8000-000000000050', 1, 1, '00000000-0000-7000-8000-000000000023', 'w1', now());
select pg_temp.expect_violation('I7 two attempts in flight for one delivery', $s$
  insert into delivery_attempts (notification_id, delivery_id, attempt_no, lease_epoch, registration_id, worker_id, started_at)
  values ('00000000-0000-7000-8000-000000000040', '00000000-0000-7000-8000-000000000050', 2, 2, '00000000-0000-7000-8000-000000000023', 'w2', now()) $s$);

-- I13: one ledger row per delivery, ever.
insert into usage_ledger (notification_id, delivery_id, environment_id)
  values ('00000000-0000-7000-8000-000000000040', '00000000-0000-7000-8000-000000000050', '00000000-0000-7000-8000-000000000003');
select pg_temp.expect_violation('I13 delivery billed twice', $s$
  insert into usage_ledger (notification_id, delivery_id, environment_id)
  values ('00000000-0000-7000-8000-000000000040', '00000000-0000-7000-8000-000000000050', '00000000-0000-7000-8000-000000000003') $s$);

-- Interaction events always have a partition to land in.
insert into interaction_events (id, environment_id, installation_id, kind, occurred_at)
  values ('00000000-0000-7000-8000-000000000060', '00000000-0000-7000-8000-000000000003', '00000000-0000-7000-8000-000000000010', 'opened', now());

rollback;
