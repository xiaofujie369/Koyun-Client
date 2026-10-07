INSERT INTO tenants(id, slug, name, panel_type) VALUES
    ('alpha', 'alpha', 'Alpha', 'mock'), ('beta', 'beta', 'Beta', 'mock');
INSERT INTO managed_accounts(id, tenant_id, external_user_id, email_normalized, panel_credential_encrypted) VALUES
    ('00000000-0000-0000-0000-000000000001', 'alpha', 'one', 'one@example.com', '\x00'),
    ('00000000-0000-0000-0000-000000000002', 'beta', 'two', 'two@example.com', '\x00'),
    ('00000000-0000-0000-0000-000000000003', 'alpha', 'three', 'three@example.com', '\x00');
INSERT INTO devices(id, tenant_id, account_id, installation_hash, name, platform) VALUES
    ('10000000-0000-0000-0000-000000000001', 'alpha', '00000000-0000-0000-0000-000000000001', decode(repeat('01', 32), 'hex'), 'A', 'linux'),
    ('10000000-0000-0000-0000-000000000002', 'beta', '00000000-0000-0000-0000-000000000002', decode(repeat('02', 32), 'hex'), 'B', 'linux');

DO $$ BEGIN
    BEGIN
        INSERT INTO devices(id, tenant_id, account_id, installation_hash, name, platform)
        VALUES ('10000000-0000-0000-0000-000000000003', 'alpha', '00000000-0000-0000-0000-000000000002', decode(repeat('03', 32), 'hex'), 'Invalid', 'linux');
        RAISE EXCEPTION 'Cross-tenant device reference was accepted';
    EXCEPTION WHEN foreign_key_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO sessions(id, tenant_id, account_id, device_id, access_token_hash, access_expires_at, expires_at)
        VALUES ('20000000-0000-0000-0000-000000000001', 'alpha', '00000000-0000-0000-0000-000000000003',
            '10000000-0000-0000-0000-000000000001', decode(repeat('01', 32), 'hex'), now() + interval '15 minutes', now() + interval '1 day');
        RAISE EXCEPTION 'Same-tenant wrong-account session reference was accepted';
    EXCEPTION WHEN foreign_key_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO managed_accounts(id, tenant_id, external_user_id, email_normalized, panel_credential_encrypted)
        VALUES ('00000000-0000-0000-0000-000000000009', 'alpha', 'one', 'duplicate@example.com', '\x00');
        RAISE EXCEPTION 'Duplicate external account was accepted';
    EXCEPTION WHEN unique_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO brands(id, tenant_id, name, app_name, allow_local_mode)
        VALUES ('30000000-0000-0000-0000-000000000001', 'alpha', 'Invalid', 'Invalid', false);
        RAISE EXCEPTION 'Brand disabled local mode';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO feature_flags VALUES ('alpha', 'local_mode', false);
        RAISE EXCEPTION 'Feature flag disabled local mode';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO webhook_receipts(tenant_id, event_id) VALUES ('alpha', 'replay'), ('alpha', 'replay');
        RAISE EXCEPTION 'Webhook replay was accepted';
    EXCEPTION WHEN unique_violation THEN NULL;
    END;
    INSERT INTO webhook_receipts(tenant_id, event_id) VALUES ('alpha', 'same-event'), ('beta', 'same-event');
END $$;

CREATE ROLE platform_isolation_test NOSUPERUSER NOBYPASSRLS;
GRANT USAGE ON SCHEMA public TO platform_isolation_test;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO platform_isolation_test;
SET LOCAL ROLE platform_isolation_test;

DO $$ BEGIN
    IF (SELECT count(*) FROM devices) <> 0 THEN
        RAISE EXCEPTION 'Unscoped connection sees tenant data';
    END IF;
END $$;

SELECT set_config('app.tenant_id', 'alpha', true);
DO $$ BEGIN
    IF (SELECT count(*) FROM devices) <> 1 THEN
        RAISE EXCEPTION 'Tenant row filtering failed';
    END IF;
    IF (SELECT count(*) FROM managed_accounts) <> 2 THEN
        RAISE EXCEPTION 'Account row filtering failed';
    END IF;
    UPDATE devices SET name = 'illegally changed' WHERE tenant_id = 'beta';
    IF FOUND THEN RAISE EXCEPTION 'Foreign tenant row was updated'; END IF;
    BEGIN
        INSERT INTO notices(id, tenant_id, title, body)
        VALUES ('40000000-0000-0000-0000-000000000001', 'beta', 'forbidden', 'forbidden');
        RAISE EXCEPTION 'Foreign tenant row was inserted';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    UPDATE devices SET name = 'allowed change' WHERE tenant_id = 'alpha';
    IF NOT FOUND THEN RAISE EXCEPTION 'Own tenant update was rejected'; END IF;
END $$;

SELECT set_config('app.tenant_id', 'beta', true);
DO $$ BEGIN
    IF (SELECT count(*) FROM devices) <> 1 THEN RAISE EXCEPTION 'Tenant switch leaked rows'; END IF;
    IF (SELECT name FROM devices LIMIT 1) <> 'B' THEN RAISE EXCEPTION 'Foreign update mutated beta'; END IF;
END $$;
RESET ROLE;

DO $$ DECLARE protected_count integer; BEGIN
    SELECT count(*) INTO protected_count FROM pg_class
    WHERE relnamespace = 'public'::regnamespace AND relkind = 'r' AND relrowsecurity AND relforcerowsecurity;
    IF protected_count <> 18 THEN RAISE EXCEPTION 'Expected 18 RLS-protected tables, got %', protected_count; END IF;
END $$;
SELECT 'Database isolation and integrity checks passed' AS result;
