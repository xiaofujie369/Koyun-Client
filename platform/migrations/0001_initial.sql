CREATE TABLE tenants (
    id text PRIMARY KEY CHECK (id ~ '^[A-Za-z0-9_-]{1,64}$'),
    slug text NOT NULL UNIQUE,
    name text NOT NULL,
    status text NOT NULL DEFAULT 'disabled' CHECK (status IN ('active', 'disabled', 'suspended')),
    panel_type text NOT NULL,
    panel_base_url text,
    panel_config_encrypted bytea,
    default_device_limit integer NOT NULL DEFAULT 2 CHECK (default_device_limit > 0),
    offline_grace_seconds integer NOT NULL DEFAULT 259200 CHECK (offline_grace_seconds >= 0),
    policy_version bigint NOT NULL DEFAULT 1 CHECK (policy_version > 0),
    api_endpoints jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(api_endpoints) = 'array'),
    ws_endpoint text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE tenant_licenses (
    id uuid PRIMARY KEY,
    tenant_id text NOT NULL UNIQUE REFERENCES tenants(id),
    status text NOT NULL CHECK (status IN ('trial', 'active', 'grace', 'suspended', 'expired', 'revoked')),
    license_type text NOT NULL DEFAULT 'internal',
    starts_at timestamptz NOT NULL,
    expires_at timestamptz,
    grace_until timestamptz,
    max_users bigint CHECK (max_users >= 0),
    max_active_devices bigint CHECK (max_active_devices >= 0),
    max_brands integer CHECK (max_brands >= 0),
    allow_linux boolean NOT NULL DEFAULT true,
    allow_windows boolean NOT NULL DEFAULT false,
    allow_android boolean NOT NULL DEFAULT false,
    allow_realtime boolean NOT NULL DEFAULT false,
    allow_white_label boolean NOT NULL DEFAULT false,
    allow_custom_domain boolean NOT NULL DEFAULT false,
    billing_customer_id text,
    billing_plan text,
    billing_cycle text,
    next_billing_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (expires_at IS NULL OR expires_at > starts_at),
    CHECK (status <> 'grace' OR grace_until IS NOT NULL)
);

CREATE TABLE release_channels (
    id uuid NOT NULL,
    tenant_id text NOT NULL REFERENCES tenants(id),
    name text NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, name)
);

CREATE TABLE brands (
    id uuid NOT NULL,
    tenant_id text NOT NULL REFERENCES tenants(id),
    name text NOT NULL,
    app_name text NOT NULL,
    logo_url text,
    icon_url text,
    theme_config jsonb NOT NULL DEFAULT '{}',
    website_url text,
    support_url text,
    privacy_url text,
    terms_url text,
    allow_local_mode boolean NOT NULL DEFAULT true CHECK (allow_local_mode),
    allow_other_tenants boolean NOT NULL DEFAULT true,
    windows_app_id text,
    windows_product_name text,
    android_application_id text,
    release_channel_id uuid,
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, release_channel_id) REFERENCES release_channels(tenant_id, id)
);

CREATE TABLE managed_accounts (
    id uuid NOT NULL,
    tenant_id text NOT NULL REFERENCES tenants(id),
    external_user_id text NOT NULL CHECK (length(external_user_id) BETWEEN 1 AND 128),
    email_normalized text NOT NULL,
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
    panel_credential_encrypted bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_login_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, external_user_id)
);

CREATE TABLE devices (
    id uuid NOT NULL,
    tenant_id text NOT NULL,
    account_id uuid NOT NULL,
    installation_hash bytea NOT NULL CHECK (octet_length(installation_hash) = 32),
    name text NOT NULL,
    platform text NOT NULL CHECK (platform IN ('linux', 'windows', 'android')),
    capabilities jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(capabilities) = 'array'),
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, account_id, id),
    UNIQUE (tenant_id, account_id, installation_hash),
    FOREIGN KEY (tenant_id, account_id) REFERENCES managed_accounts(tenant_id, id)
);
CREATE INDEX devices_active_account ON devices(tenant_id, account_id) WHERE revoked_at IS NULL;

CREATE TABLE sessions (
    id uuid NOT NULL,
    tenant_id text NOT NULL,
    account_id uuid NOT NULL,
    device_id uuid NOT NULL,
    access_token_hash bytea NOT NULL CHECK (octet_length(access_token_hash) = 32),
    access_expires_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, access_token_hash),
    FOREIGN KEY (tenant_id, account_id, device_id) REFERENCES devices(tenant_id, account_id, id),
    CHECK (access_expires_at <= expires_at)
);
CREATE INDEX sessions_device ON sessions(tenant_id, device_id) WHERE revoked_at IS NULL;

CREATE TABLE refresh_tokens (
    id uuid NOT NULL,
    tenant_id text NOT NULL,
    session_id uuid NOT NULL,
    token_hash bytea NOT NULL CHECK (octet_length(token_hash) = 32),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, token_hash),
    FOREIGN KEY (tenant_id, session_id) REFERENCES sessions(tenant_id, id)
);
CREATE INDEX refresh_tokens_session ON refresh_tokens(tenant_id, session_id);

CREATE TABLE entitlements (
    tenant_id text NOT NULL,
    account_id uuid NOT NULL,
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    quota_bytes bigint NOT NULL CHECK (quota_bytes >= 0),
    upload_bytes bigint NOT NULL CHECK (upload_bytes >= 0),
    download_bytes bigint NOT NULL CHECK (download_bytes >= 0),
    device_limit integer CHECK (device_limit >= 0),
    expires_at timestamptz,
    last_verified_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, account_id),
    FOREIGN KEY (tenant_id, account_id) REFERENCES managed_accounts(tenant_id, id)
);

CREATE TABLE managed_profiles (
    id uuid NOT NULL,
    tenant_id text NOT NULL,
    account_id uuid NOT NULL,
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    etag text,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'active', 'suspended', 'error')),
    last_sync_at timestamptz,
    last_success_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, account_id),
    FOREIGN KEY (tenant_id, account_id) REFERENCES managed_accounts(tenant_id, id)
);

CREATE TABLE profile_cache (
    tenant_id text NOT NULL,
    profile_id uuid NOT NULL,
    version bigint NOT NULL CHECK (version > 0),
    content_encrypted bytea NOT NULL CHECK (octet_length(content_encrypted) <= 8388736),
    etag text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, profile_id, version),
    FOREIGN KEY (tenant_id, profile_id) REFERENCES managed_profiles(tenant_id, id)
);

CREATE TABLE realtime_events (
    id uuid NOT NULL,
    tenant_id text NOT NULL REFERENCES tenants(id),
    account_id uuid,
    type text NOT NULL CHECK (type IN ('profile.changed', 'entitlement.changed', 'account.disabled',
        'device.revoked', 'notice.published', 'app.policy.changed', 'tenant.policy.changed')),
    version bigint NOT NULL CHECK (version > 0),
    payload jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(payload) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, account_id) REFERENCES managed_accounts(tenant_id, id)
);
CREATE INDEX realtime_events_outbox ON realtime_events(tenant_id, created_at) WHERE published_at IS NULL;

CREATE TABLE releases (
    id uuid NOT NULL,
    tenant_id text NOT NULL,
    channel_id uuid NOT NULL,
    version text NOT NULL,
    platform text NOT NULL CHECK (platform IN ('linux', 'windows', 'android')),
    architecture text NOT NULL,
    download_url text NOT NULL CHECK (download_url LIKE 'https://%'),
    sha256 bytea NOT NULL CHECK (octet_length(sha256) = 32),
    size_bytes bigint NOT NULL CHECK (size_bytes > 0),
    manifest_signature bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, channel_id, version, platform, architecture),
    FOREIGN KEY (tenant_id, channel_id) REFERENCES release_channels(tenant_id, id)
);

CREATE TABLE notices (
    id uuid NOT NULL,
    tenant_id text NOT NULL REFERENCES tenants(id),
    title text NOT NULL,
    body text NOT NULL,
    published_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id)
);

CREATE TABLE admin_users (
    id uuid PRIMARY KEY,
    email_normalized text NOT NULL UNIQUE,
    password_hash text NOT NULL,
    disabled_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE admin_roles (
    id uuid PRIMARY KEY,
    admin_user_id uuid NOT NULL REFERENCES admin_users(id),
    tenant_id text REFERENCES tenants(id),
    role text NOT NULL CHECK (role IN ('platform_super_admin', 'tenant_admin', 'tenant_operator', 'tenant_support')),
    CHECK ((role = 'platform_super_admin') = (tenant_id IS NULL)),
    UNIQUE NULLS NOT DISTINCT (admin_user_id, tenant_id, role)
);

CREATE TABLE admin_audit_logs (
    id uuid PRIMARY KEY,
    tenant_id text REFERENCES tenants(id),
    admin_user_id uuid NOT NULL REFERENCES admin_users(id),
    action text NOT NULL,
    target text NOT NULL,
    result text NOT NULL CHECK (result IN ('success', 'denied', 'failure')),
    ip inet,
    request_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX admin_audit_tenant_time ON admin_audit_logs(tenant_id, created_at);

CREATE TABLE tenant_api_keys (
    id uuid NOT NULL,
    tenant_id text NOT NULL REFERENCES tenants(id),
    key_hash bytea NOT NULL CHECK (octet_length(key_hash) = 32),
    webhook_secret_encrypted bytea NOT NULL,
    label text NOT NULL,
    expires_at timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, key_hash)
);

CREATE TABLE feature_flags (
    tenant_id text NOT NULL REFERENCES tenants(id),
    name text NOT NULL,
    enabled boolean NOT NULL,
    PRIMARY KEY (tenant_id, name),
    CHECK (name <> 'local_mode' OR enabled)
);

CREATE TABLE webhook_receipts (
    tenant_id text NOT NULL REFERENCES tenants(id),
    event_id text NOT NULL CHECK (length(event_id) BETWEEN 1 AND 128),
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, event_id)
);

DO $$
DECLARE scoped_table text;
BEGIN
    FOREACH scoped_table IN ARRAY ARRAY[
        'tenant_licenses', 'release_channels', 'brands', 'managed_accounts', 'devices', 'sessions',
        'refresh_tokens', 'entitlements', 'managed_profiles', 'profile_cache', 'realtime_events',
        'releases', 'notices', 'admin_roles', 'admin_audit_logs', 'tenant_api_keys', 'feature_flags', 'webhook_receipts'
    ] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', scoped_table);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', scoped_table);
        EXECUTE format('CREATE POLICY tenant_scope ON %I USING (tenant_id = current_setting(''app.tenant_id'', true)) WITH CHECK (tenant_id = current_setting(''app.tenant_id'', true))', scoped_table);
    END LOOP;
END $$;
