SELECT set_config('app.tenant_id', 'alpha', true);
INSERT INTO sessions(id, tenant_id, account_id, device_id, access_token_hash, access_expires_at, expires_at)
VALUES ('20000000-0000-0000-0000-000000000001', 'alpha', '00000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001', decode(repeat('11', 32), 'hex'), now() + interval '15 minutes', now() + interval '1 day');
INSERT INTO refresh_tokens(id, tenant_id, session_id, token_hash, expires_at)
VALUES ('50000000-0000-0000-0000-000000000001', 'alpha', '20000000-0000-0000-0000-000000000001', decode(repeat('12', 32), 'hex'), now() + interval '1 day');

SET LOCAL ROLE platform_isolation_test;
DO $$ DECLARE result text; BEGIN
    PERFORM set_config('app.tenant_id', 'beta', true);
    result := rotate_refresh_token(decode(repeat('12', 32), 'hex'), decode(repeat('13', 32), 'hex'),
        '50000000-0000-0000-0000-000000000002', decode(repeat('14', 32), 'hex'), now());
    IF result <> 'invalid' THEN RAISE EXCEPTION 'Foreign tenant refreshed a session'; END IF;

    PERFORM set_config('app.tenant_id', 'alpha', true);
    result := rotate_refresh_token(decode(repeat('12', 32), 'hex'), decode(repeat('13', 32), 'hex'),
        '50000000-0000-0000-0000-000000000002', decode(repeat('14', 32), 'hex'), now());
    IF result <> 'ok' THEN RAISE EXCEPTION 'Valid refresh failed: %', result; END IF;
    IF (SELECT consumed_at FROM refresh_tokens WHERE token_hash = decode(repeat('12', 32), 'hex')) IS NULL THEN
        RAISE EXCEPTION 'Old refresh token not consumed';
    END IF;
    IF (SELECT access_token_hash FROM sessions WHERE id = '20000000-0000-0000-0000-000000000001') <> decode(repeat('13', 32), 'hex') THEN
        RAISE EXCEPTION 'Access token was not rotated';
    END IF;
    result := rotate_refresh_token(decode(repeat('12', 32), 'hex'), decode(repeat('15', 32), 'hex'),
        '50000000-0000-0000-0000-000000000003', decode(repeat('16', 32), 'hex'), now());
    IF result <> 'replayed' THEN RAISE EXCEPTION 'Replay was accepted'; END IF;
    IF (SELECT revoked_at FROM sessions WHERE id = '20000000-0000-0000-0000-000000000001') IS NULL THEN
        RAISE EXCEPTION 'Replay did not revoke the session family';
    END IF;
    result := rotate_refresh_token(decode(repeat('14', 32), 'hex'), decode(repeat('17', 32), 'hex'),
        '50000000-0000-0000-0000-000000000004', decode(repeat('18', 32), 'hex'), now());
    IF result <> 'invalid' THEN RAISE EXCEPTION 'Rotated token survived family revocation'; END IF;
END $$;
RESET ROLE;
INSERT INTO sessions(id, tenant_id, account_id, device_id, access_token_hash, access_expires_at, expires_at)
VALUES
    ('20000000-0000-0000-0000-000000000002', 'alpha', '00000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001', decode(repeat('21', 32), 'hex'), now() + interval '15 minutes', now() + interval '1 day'),
    ('20000000-0000-0000-0000-000000000003', 'alpha', '00000000-0000-0000-0000-000000000001',
        '10000000-0000-0000-0000-000000000001', decode(repeat('31', 32), 'hex'), now() + interval '15 minutes', now() + interval '1 day');
INSERT INTO refresh_tokens(id, tenant_id, session_id, token_hash, expires_at)
VALUES
    ('50000000-0000-0000-0000-000000000005', 'alpha', '20000000-0000-0000-0000-000000000002', decode(repeat('22', 32), 'hex'), now() - interval '1 hour'),
    ('50000000-0000-0000-0000-000000000006', 'alpha', '20000000-0000-0000-0000-000000000003', decode(repeat('32', 32), 'hex'), now() + interval '1 day');
SET LOCAL ROLE platform_isolation_test;
DO $$ DECLARE result text; BEGIN
    result := rotate_refresh_token(decode(repeat('22', 32), 'hex'), decode(repeat('23', 32), 'hex'),
        '50000000-0000-0000-0000-000000000007', decode(repeat('24', 32), 'hex'), now());
    IF result <> 'invalid' THEN RAISE EXCEPTION 'Expired refresh token was accepted'; END IF;
    UPDATE devices SET revoked_at = now() WHERE tenant_id = 'alpha' AND id = '10000000-0000-0000-0000-000000000001';
    result := rotate_refresh_token(decode(repeat('32', 32), 'hex'), decode(repeat('33', 32), 'hex'),
        '50000000-0000-0000-0000-000000000008', decode(repeat('34', 32), 'hex'), now());
    IF result <> 'disabled' THEN RAISE EXCEPTION 'Revoked device refreshed a session'; END IF;
    IF (SELECT revoked_at FROM sessions WHERE id = '20000000-0000-0000-0000-000000000003') IS NULL THEN
        RAISE EXCEPTION 'Revoked device session was not revoked';
    END IF;
END $$;
RESET ROLE;
SELECT 'Refresh rotation and replay revocation checks passed' AS result;
