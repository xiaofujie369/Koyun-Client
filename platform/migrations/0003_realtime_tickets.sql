CREATE TABLE realtime_tickets (
    tenant_id text NOT NULL,
    token_hash bytea NOT NULL CHECK (octet_length(token_hash)=32),
    session_id uuid NOT NULL,
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    PRIMARY KEY (tenant_id,token_hash),
    FOREIGN KEY (tenant_id,session_id) REFERENCES sessions(tenant_id,id)
);
ALTER TABLE realtime_tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE realtime_tickets FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_scope ON realtime_tickets
USING (tenant_id=current_setting('app.tenant_id',true))
WITH CHECK (tenant_id=current_setting('app.tenant_id',true));
