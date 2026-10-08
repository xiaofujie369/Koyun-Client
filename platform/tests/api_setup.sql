CREATE ROLE platform_api_runtime LOGIN PASSWORD 'local-disposable-test-only' NOSUPERUSER NOBYPASSRLS;
GRANT CONNECT ON DATABASE platform_api_test TO platform_api_runtime;
\ir ../runtime-grants.sql
INSERT INTO tenants(id,slug,name,status,panel_type) VALUES ('demo','demo','Demo','active','mock'),('other','other','Other','active','mock');
INSERT INTO tenant_licenses(id,tenant_id,status,starts_at)
VALUES ('10000000-0000-0000-0000-000000000001','demo','active',now()-interval '1 hour'),
('10000000-0000-0000-0000-000000000002','other','active',now()-interval '1 hour');
UPDATE tenant_licenses SET allow_realtime=true;
INSERT INTO brands(id,tenant_id,name,app_name) VALUES ('20000000-0000-0000-0000-000000000001','demo','Demo Brand','Koyun Client');
