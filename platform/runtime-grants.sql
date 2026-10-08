GRANT USAGE ON SCHEMA public TO platform_api_runtime;
GRANT SELECT (id,name,status,panel_type,panel_base_url,default_device_limit), UPDATE (id) ON tenants TO platform_api_runtime;
GRANT SELECT ON tenant_licenses TO platform_api_runtime;
GRANT SELECT, INSERT, UPDATE ON managed_accounts, devices, sessions, refresh_tokens TO platform_api_runtime;
GRANT EXECUTE ON FUNCTION rotate_refresh_token(bytea,bytea,uuid,bytea,timestamptz) TO platform_api_runtime;
