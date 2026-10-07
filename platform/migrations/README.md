# PostgreSQL 迁移

`0001_initial.sql` 建立首版 20 张表，使用 PostgreSQL 18。
托管业务的设备、会话、配置等引用包含 tenant_id，会话还必须引用属于同一账户的设备。
API Key 与刷新令牌只保存 32 字节哈希；面板凭据、Webhook 密钥和订阅缓存保存加密字节。
字段类型只约束存储形态，AEAD 加密及随机令牌生成仍需由服务层负责。

18 张租户表启用并强制 RLS。正常服务连接必须使用非 superuser、无 BYPASSRLS 的数据库角色，
每个事务使用 `set_config('app.tenant_id', tenant_id, true)` 设置范围；事务结束自动清除。
表所有者与运行时角色分离，不向运行时角色授予 TRUNCATE 或更改表/策略权限。
RLS 只是额外边界；请求身份认证、租户管理员 RBAC 和查询参数绑定仍必须在服务层落实。

tenants 为平台级目录，admin_users 为平台管理员身份表。平台超级管理员的角色和审计记录
允许 tenant_id 为空；这类行不会通过普通租户连接的 RLS，需要独立受限的管理访问路径。
公共目录接口只能输出允许公开的列，不能直接序列化 tenants 中的加密配置。

首次数据库测试在一次事务内执行迁移、测试，然后回滚，不写入现有业务数据库：

```sh
psql "$TEST_DATABASE_URL" -v ON_ERROR_STOP=1 \
  -c BEGIN -f platform/migrations/0001_initial.sql \
  -f platform/migrations/0002_refresh_rotation.sql \
  -f platform/tests/database.sql -f platform/tests/refresh_rotation.sql -c ROLLBACK
```

此测试需要可创建临时测试角色的隔离数据库管理员连接；测试会切换至 NOSUPERUSER /
NOBYPASSRLS 角色验证读取、写入及缺少租户上下文时的行为，不能仅以 superuser 测 RLS。
不要把 TEST_DATABASE_URL 指向生产环境。后续实际部署通过服务迁移执行器记录版本，
已部署迁移只追加，不修改历史文件。

`rotate_refresh_token` 以会话行锁串行化同一会话的刷新，然后锁定令牌行，
在一个事务中消费旧令牌、替换访问令牌哈希并创建新刷新令牌。
检测重放后返回 `replayed` 并撤销会话族。调用方必须提交此事务再返回 401，
不能用异常回滚撤销操作。SQL 函数使用调用者权限并受租户 RLS 约束。
