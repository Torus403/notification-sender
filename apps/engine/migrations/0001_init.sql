-- 0001: core schema for the push delivery engine (spec section 4).
-- Rules that Postgres can enforce live here as constraints, not in application code.

-- ===== Tenancy and configuration =====
create table organizations (
  id uuid primary key,
  name text not null,
  created_at timestamptz not null default now()
);

create table projects (
  id uuid primary key,
  organization_id uuid not null references organizations(id),
  name text not null,
  created_at timestamptz not null default now()
);

create table environments (
  id uuid primary key,
  project_id uuid not null references projects(id),
  kind text not null check (kind in ('development','staging','production')),
  dek_wrapped bytea not null,          -- KMS-wrapped data key: tokens, payloads, identity secret
  identity_secret_ct bytea not null,   -- AES-GCM under the environment data key
  paused_at timestamptz,
  created_at timestamptz not null default now(),
  unique (project_id, kind)
);

create table apps (
  id uuid primary key,
  project_id uuid not null references projects(id),
  platform text not null check (platform in ('ios','android')),
  bundle_id text not null,             -- iOS bundle id or Android package name
  unique (project_id, platform, bundle_id)
);

create table provider_credentials (
  id uuid primary key,
  organization_id uuid not null references organizations(id),
  kind text not null check (kind in ('apns_token_key','fcm_service_account')),
  apns_team_id text,
  apns_key_id text,
  fcm_project_id text,
  fcm_client_email text,
  secret_ct bytea not null,            -- .p8 or service-account JSON, AES-256-GCM
  dek_wrapped bytea not null,          -- wrapped by the credentials KMS key, context {org, cred}
  generation int not null default 1,   -- bumped on rotation; pools for older generations drain
  state text not null default 'unverified'
    check (state in ('unverified','valid','failing','revoked')),
  last_error text,
  last_verified_at timestamptz,
  created_at timestamptz not null default now(),
  check (kind <> 'apns_token_key' or (apns_team_id is not null and apns_key_id is not null)),
  check (kind <> 'fcm_service_account' or fcm_project_id is not null)
);

create table app_configs (
  environment_id uuid not null references environments(id),
  app_id uuid not null references apps(id),
  credential_id uuid references provider_credentials(id),
  next_credential_id uuid references provider_credentials(id),
  paused_at timestamptz,
  breaker_open_until timestamptz,
  primary key (environment_id, app_id)
);

create table app_sounds (
  app_id uuid not null references apps(id),
  name text not null,
  ios_file text,
  android_channel_id text,
  primary key (app_id, name)
);

create table api_keys (
  id uuid primary key,
  environment_id uuid not null references environments(id),
  kind text not null check (kind in ('secret','publishable')),
  display_prefix text not null,
  key_hash bytea not null unique,      -- sha256(full key)
  scopes text[] not null,
  created_at timestamptz not null default now(),
  revoked_at timestamptz
);

create table audit_log (
  id uuid primary key,
  organization_id uuid not null,
  actor text not null,
  action text not null,
  target text not null,
  detail jsonb not null default '{}',
  at timestamptz not null default now()
);

-- ===== Device identity =====
create table installations (
  id uuid primary key,
  environment_id uuid not null,
  app_id uuid not null,
  token_hash bytea not null unique,    -- sha256(installation token)
  source text not null default 'sdk' check (source in ('sdk','import')),
  sdk_version text,
  app_version text,
  os_version text,
  device_model text,
  permission text not null default 'unknown'
    check (permission in ('unknown','not_determined','denied','authorized','provisional','ephemeral')),
  permission_observed_at timestamptz,
  last_seen_at timestamptz not null default now(),
  deleted_at timestamptz,
  created_at timestamptz not null default now(),
  foreign key (environment_id, app_id) references app_configs (environment_id, app_id)
);

create table push_registrations (
  id uuid primary key,                 -- the registration version
  installation_id uuid not null references installations(id),
  environment_id uuid not null,
  app_id uuid not null,
  provider text not null check (provider in ('apns','fcm')),
  apns_env text check (apns_env in ('sandbox','production')),
  token_ct bytea not null,
  token_digest bytea not null,         -- HMAC-SHA256(environment key, token)
  state text not null default 'active'
    check (state in ('active','superseded','invalid','deleted')),
  ended_reason text,
  ended_at timestamptz,
  created_at timestamptz not null default now(),
  check ((provider = 'apns') = (apns_env is not null)),
  check ((state = 'active') = (ended_at is null))
);
create unique index reg_one_active_per_installation
  on push_registrations (installation_id) where state = 'active';
create unique index reg_one_active_owner_per_token
  on push_registrations (environment_id, app_id, token_digest) where state = 'active';

create table user_bindings (
  id uuid primary key,
  installation_id uuid not null references installations(id),
  environment_id uuid not null,
  external_user_id text not null check (octet_length(external_user_id) between 1 and 256),
  method text not null check (method in ('identity_token','server_api','import')),
  bound_at timestamptz not null default now(),
  unbound_at timestamptz,
  unbound_reason text
);
create unique index binding_one_active_per_installation
  on user_bindings (installation_id) where unbound_at is null;
create index binding_active_by_user
  on user_bindings (environment_id, external_user_id) where unbound_at is null;

create table identity_token_uses (
  environment_id uuid not null,
  jti text not null,
  expires_at timestamptz not null,
  primary key (environment_id, jti)
);

-- ===== Templates =====
create table templates (
  id uuid primary key,
  project_id uuid not null references projects(id),
  key text not null,
  unique (project_id, key)
);

create table template_versions (
  id uuid primary key,
  template_id uuid not null references templates(id),
  version int not null,
  content jsonb not null,
  variables jsonb not null,
  created_at timestamptz not null default now(),
  unique (template_id, version)
);

create table template_publications (
  template_id uuid not null references templates(id),
  environment_id uuid not null references environments(id),
  version_id uuid not null references template_versions(id),
  disabled_at timestamptz,
  primary key (template_id, environment_id)
);

-- ===== Delivery =====
create table idempotency_keys (
  environment_id uuid not null,
  key text not null check (octet_length(key) between 1 and 255),
  request_hash bytea not null,
  notification_id uuid not null,
  expires_at timestamptz not null,
  primary key (environment_id, key)
);
create index idempotency_keys_expiry on idempotency_keys (expires_at);

create table notifications (
  id uuid primary key,
  environment_id uuid not null,
  target_kind text not null check (target_kind in ('user','installation')),
  target_value text not null,
  kind text not null check (kind in ('alert','background')),
  priority text not null check (priority in ('high','normal')),
  template_version_id uuid,
  collapse_key text not null check (octet_length(collapse_key) <= 64),
  expires_at timestamptz not null,
  status text not null check (status in ('processing','completed','cancelled')),
  outcome text,
  is_test boolean not null default false,
  deliveries_total smallint not null check (deliveries_total >= 0),
  deliveries_open smallint not null check (deliveries_open between 0 and deliveries_total),
  created_at timestamptz not null default now(),
  completed_at timestamptz,
  check ((status = 'processing') = (completed_at is null))
);
create index notifications_stuck on notifications (created_at) where status = 'processing';

create table notification_payloads (
  notification_id uuid primary key references notifications(id),
  environment_id uuid not null,
  content_ct bytea not null,
  purge_after timestamptz not null
);
create index notification_payloads_purge on notification_payloads (purge_after);

create table deliveries (
  notification_id uuid not null references notifications(id),
  id uuid not null,
  environment_id uuid not null,
  installation_id uuid not null,
  app_id uuid not null,
  provider text not null check (provider in ('apns','fcm')),
  state text not null default 'pending' check (state in
    ('pending','claimed','provider_accepted','failed','unknown','expired','suppressed','cancelled')),
  reason text,
  attempt_count smallint not null default 0,   -- every attempt
  budget_used smallint not null default 0,     -- attempts that count toward the retry limit
  ambiguous_count smallint not null default 0,
  next_attempt_at timestamptz not null,
  expires_at timestamptz not null,
  lease_epoch bigint not null default 0,
  lease_owner text,
  lease_until timestamptz,
  enqueued_at timestamptz,
  provider_message_id text,
  created_at timestamptz not null default now(),
  finished_at timestamptz,
  primary key (notification_id, id),
  unique (notification_id, installation_id),
  check ((state = 'claimed') = (lease_until is not null)),
  check ((state in ('pending','claimed')) = (finished_at is null))
);
create index deliveries_due on deliveries (next_attempt_at) where state = 'pending';
create index deliveries_leases on deliveries (lease_until) where state = 'claimed';

create table delivery_attempts (
  notification_id uuid not null,
  delivery_id uuid not null,
  attempt_no smallint not null check (attempt_no >= 1),
  lease_epoch bigint not null,
  registration_id uuid not null references push_registrations(id),
  worker_id text not null,
  state text not null default 'in_flight' check (state in
    ('in_flight','accepted','rejected','retryable','not_sent','indeterminate','abandoned')),
  http_status smallint,
  provider_reason text,
  normalized_reason text,
  provider_message_id text,
  started_at timestamptz not null,
  finished_at timestamptz,
  latency_us int,
  primary key (notification_id, delivery_id, attempt_no),
  foreign key (notification_id, delivery_id) references deliveries (notification_id, id)
);
create unique index attempts_one_in_flight_per_delivery
  on delivery_attempts (notification_id, delivery_id) where state = 'in_flight';

create table usage_ledger (
  notification_id uuid not null,
  delivery_id uuid not null,
  environment_id uuid not null,
  billed_at timestamptz not null default now(),
  primary key (notification_id, delivery_id),
  foreign key (notification_id, delivery_id) references deliveries (notification_id, id)
);
create index usage_ledger_rollup on usage_ledger (environment_id, billed_at);

create table interaction_events (
  id uuid not null,
  environment_id uuid not null,
  installation_id uuid not null,
  notification_id uuid,
  delivery_id uuid,
  kind text not null check (kind in ('received','opened','action','permission_changed')),
  occurred_at timestamptz not null,
  received_at timestamptz not null default now(),
  props jsonb not null default '{}',
  primary key (received_at, id)
) partition by range (received_at);
-- Daily partitions are created ahead by the sweeper (spec section 10). The default
-- partition catches anything outside them so an insert never fails.
create table interaction_events_default partition of interaction_events default;
