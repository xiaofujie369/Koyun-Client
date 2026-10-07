CREATE FUNCTION rotate_refresh_token(
    p_token_hash bytea,
    p_next_access_hash bytea,
    p_next_refresh_id uuid,
    p_next_refresh_hash bytea,
    p_now timestamptz
) RETURNS text
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    scoped_tenant text := current_setting('app.tenant_id', true);
    current_session sessions%ROWTYPE;
    current_token refresh_tokens%ROWTYPE;
BEGIN
    SELECT s.* INTO current_session FROM sessions s
    JOIN refresh_tokens r ON r.tenant_id = s.tenant_id AND r.session_id = s.id
    WHERE r.tenant_id = scoped_tenant AND r.token_hash = p_token_hash
    FOR UPDATE OF s;
    IF NOT FOUND THEN RETURN 'invalid'; END IF;

    SELECT * INTO current_token FROM refresh_tokens
    WHERE tenant_id = scoped_tenant AND token_hash = p_token_hash
    FOR UPDATE;
    IF NOT FOUND THEN RETURN 'invalid'; END IF;
    IF current_session.revoked_at IS NOT NULL THEN RETURN 'invalid'; END IF;

    IF current_token.consumed_at IS NOT NULL THEN
        UPDATE sessions SET revoked_at = p_now
        WHERE tenant_id = scoped_tenant AND id = current_session.id;
        RETURN 'replayed';
    END IF;

    IF current_token.expires_at <= p_now OR current_session.expires_at <= p_now THEN
        RETURN 'invalid';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM devices d JOIN managed_accounts a
        ON a.tenant_id = d.tenant_id AND a.id = d.account_id
        WHERE d.tenant_id = scoped_tenant AND d.id = current_session.device_id
        AND d.account_id = current_session.account_id AND d.revoked_at IS NULL AND a.status = 'active'
    ) THEN
        UPDATE sessions SET revoked_at = p_now
        WHERE tenant_id = scoped_tenant AND id = current_session.id;
        RETURN 'disabled';
    END IF;

    UPDATE refresh_tokens SET consumed_at = p_now
    WHERE tenant_id = scoped_tenant AND id = current_token.id;
    UPDATE sessions SET access_token_hash = p_next_access_hash,
        access_expires_at = LEAST(p_now + interval '15 minutes', expires_at)
    WHERE tenant_id = scoped_tenant AND id = current_session.id;
    INSERT INTO refresh_tokens(id, tenant_id, session_id, token_hash, expires_at, created_at)
    VALUES (p_next_refresh_id, scoped_tenant, current_session.id, p_next_refresh_hash, current_session.expires_at, p_now);
    RETURN 'ok';
END $$;
