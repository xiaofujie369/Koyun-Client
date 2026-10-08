# API 开发联调

当前服务具备登录、刷新、退出、设备注册/列表/撤销、租户用户和权益查询，
以及托管配置、版本同步与 WebSocket 通知。签名 Bootstrap、管理后台和客户端接入
仍在开发，不能作为完整 MVP 上线。

## 启动

先创建独立 PostgreSQL 数据库。数据库迁移使用独立管理凭据：

```sh
MIGRATION_DATABASE_URL='postgres://…' cargo run --manifest-path platform/Cargo.toml -- migrate
```

创建 `platform_api_runtime` LOGIN NOSUPERUSER NOBYPASSRLS 角色，单独设置随机密码，
授予目标数据库 CONNECT，并以迁移用户执行 `platform/runtime-grants.sql`。
运行账号不得拥有迁移角色或超级用户角色。生产不得执行 `tests/api_setup.sql`。
租户和许可证目前由迁移管理员配置，后台尚未完成。

服务环境变量：`DATABASE_URL`（运行角色）、`PLATFORM_MASTER_KEY`（随机 32 字节的
64 位十六进制编码）、`LISTEN_ADDRESS`（默认 `127.0.0.1:8099`）。密钥必须备份并放入
受限权限的私密配置，丢失后无法解密面板会话。执行 `client-platform serve` 启动。
`ENABLE_DEMO=true` 才启用 mock 租户；未启用的租户不出现在公开列表。
可选 `PANEL_ORIGIN_<tenant_id>` 固定面板连接 IP:端口，仍校验原域名的 TLS 证书。

仅在受控联调网络使用。公网部署前配置 TLS 反向代理和入口限流；当前单进程按 TCP
对端 IP 对登录/刷新合计限制 20 次/分钟，最多保留 4096 个对端，忽略转发 IP 头。
经过代理的请求会共享代理 IP 配额；多实例限流和可信代理配置仍需完善。

## 协议

JSON 成功结果为 `{"data":…}`；业务错误为 `{"error":{"code":"…"}}`。
响应使用 `Cache-Control: no-store` 和服务器生成的 `X-Request-Id`。
请求体最大 32 KiB。除公开列表、登录和刷新外，使用 `Authorization: Bearer <access_token>`。

| 方法与路径 | 请求/行为 |
|---|---|
| GET `/health` | 204，进程存活检查，不代表数据库健康 |
| GET `/v1/public/tenants` | 可登录的已启用租户列表 |
| POST `/v1/auth/login` | `tenant_id,email,password,device` |
| POST `/v1/auth/refresh` | `refresh_token`，一次性轮换 |
| POST `/v1/auth/logout` | 撤销当前会话族，204 |
| GET `/v1/devices` | 当前账号设备（含已撤销项） |
| POST `/v1/devices/register` | 设备对象；再次校验面板权益，返回该设备的新会话 |
| DELETE `/v1/devices/{id}` | 撤销所属设备及全部会话，204 |
| GET `/v1/tenants/{id}/me` | 当前面板身份，拒绝跨租户路径 |
| GET `/v1/tenants/{id}/entitlement` | 当前面板权益 |
| GET `/v1/managed/profiles` | 当前账号唯一托管配置的元数据 |
| GET `/v1/managed/profiles/{id}` | YAML；支持 If-None-Match 与 304 |
| GET `/v1/managed/profiles/{id}/state` | 配置状态、版本、ETag、同步时间 |
| GET `/v1/sync/state` | 配置/权益/策略版本、离线宽限、服务器时间 |
| GET `/v1/realtime/token` | 60 秒有效的一次性 WebSocket 票据 |
| WS `/v1/realtime/ws` | Authorization 使用一次性票据，不使用 access token |

设备对象包含 `installation_id`（安装时生成并持久保存的非空 UUID）、`name`、
`platform`（linux/windows/android）和可选 `capabilities` 字符串数组。
登录/注册/刷新返回 `access_token,refresh_token,expires_in,account_id,device_id,tenant_id`。
access 默认 15 分钟，会话族最长 30 天；刷新不会延长会话族寿命。
同一设备重新登录撤销旧会话；被撤销的安装 ID 不允许重新登录。
新设备额度按租户默认额度与面板额度取较小值，并受租户总设备额度约束。

客户端须串行刷新并原子保存返回的令牌；重试已消费的刷新令牌将撤销整个会话族。
401 要求重新登录；403 是明确授权拒绝；409 表示设备额度不足；429 根据 Retry-After
退避；503/502 表示平台或面板暂不可用。JSON 解析错误使用 Axum 的 4xx 响应。

## 配置与通知

访问配置或同步状态时，距上次成功同步超过 30 秒会重新查询面板身份、权益和订阅。
缓存按租户/账号/配置隔离，配置正文使用 AEAD 加密，密文绑定租户、配置 ID 和版本。
相同内容不增加版本；内容变化或从暂停状态恢复会增加版本，并写入事务内事件记录。
服务端保留最近版本用于恢复，不能代替客户端经 mihomo 验证和成功应用后的 LKG。
较早发起的请求不得覆盖较新成功响应或明确暂停状态。

面板网络故障或无效配置返回 502，保留最近成功缓存；明确身份/权益拒绝返回 401/403，
暂停托管配置。客户端只能对临时故障使用有期限的 LKG，不能对明确拒绝使用离线宽限。
YAML 响应携带 `ETag` 和 `X-Profile-Version`，仍标记 no-store 以禁止共享 HTTP 缓存。

WebSocket 连接必须先获取票据，再通过 Authorization 头发送。禁止将令牌放入 URL
或日志。连接后首个事件为 `sync.required`；随后按数据库版本变化发送
`profile.changed`、`entitlement.changed`、`tenant.policy.changed`，不发送配置正文。
当前 DatabaseRealtimeProvider 每 2 秒检查版本和会话有效性，退出、撤销、许可失效后
断开。每进程最多 256 个连接，每会话最多 2 个连接，发送和数据库检查有 5 秒超时。
连接重建必须获取新票据并先请求 sync/state；客户端仍须每 30 秒轮询以触发上游刷新
并补偿漏消息。该阶段未接入 XBoard 节点 WS，也未用 Redis 代替数据库事实来源。

## 独立数据库测试

在一次性 `platform_api_test` 数据库运行迁移，再执行 `platform/tests/api_setup.sql`。
该文件只含公开的一次性测试密码。设置运行角色的 `TEST_RUNTIME_DATABASE_URL` 后运行：

```sh
cargo test --manifest-path platform/Cargo.toml --test api -- --ignored
```

测试覆盖真实 HTTP Router 与 PostgreSQL、刷新重放撤销、并发额度、跨租户拒绝、
设备撤销、注册、退出及连接池租户范围清理；同时检查配置 ETag、失败保留缓存、
明确拒绝暂停、旧响应竞争、一次性票据及真实 WebSocket 撤销断连。
每次执行使用全新测试数据库。
