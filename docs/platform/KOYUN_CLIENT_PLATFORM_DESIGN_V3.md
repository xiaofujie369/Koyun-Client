# Koyun Client Platform
## 多机场授权 / 多租户 / FlClash 二次开发生产级设计方案 V3

> 用途：直接交给 Codex 开发。
> 基础项目：`chen08209/FlClash`
> 第一阶段目标：Linux MVP
> 后续平台：Windows、Android
> 核心定位：**不是“可云专用客户端”，而是一个可授权给多个机场使用的通用客户端平台。**

---

# 1. 项目重新定义

本项目不要再定义成：

```text
“可云自己的 FlClash 客户端”
```

正确定位：

```text
Koyun Client Platform
=
通用 FlClash 客户端
+
本地使用模式
+
多机场托管账户模式
+
多租户授权系统
+
白标品牌系统
+
面板 Adapter 系统
+
设备授权系统
+
实时推送接口
+
版本管理
```

其中“可云”只是第一个 Tenant（租户 / 机场）。

未来可以存在：

```text
Tenant A = 可云
Tenant B = 机场 B
Tenant C = 机场 C
Tenant D = 机场 D
```

每个机场都可以：

- 使用自己的 XBoard；
- 使用自己的域名；
- 使用自己的 Logo；
- 使用自己的客户端名称；
- 使用自己的颜色；
- 使用自己的套餐；
- 使用自己的设备授权策略；
- 使用自己的更新渠道；
- 使用自己的公告；
- 使用自己的 WS 实时推送；
- 使用自己的 API Endpoint；
- 使用自己的下载站。

但底层客户端代码仍然是同一套。

---

# 2. 最重要的设计原则

从第一天就必须做到：

```text
Client ≠ Koyun
Client ≠ XBoard
Client ≠ 某一家机场
```

客户端只理解：

```text
Tenant
Provider
ManagedProfile
LocalProfile
Entitlement
Device
RealtimeEvent
```

不要在业务代码中到处出现：

```text
koyunUser
koyunPlan
koyunProfile
koyunSubscription
```

统一改成：

```text
tenant
account
managedProfile
provider
entitlement
```

“可云”只存在于配置数据中，不存在于核心业务逻辑中。

---

# 3. 双模式客户端

客户端必须支持两个完全独立模式。

## 3.1 本地模式 Local Mode

无需登录任何机场。

用户可以：

- 手动添加订阅 URL；
- 扫二维码；
- 导入 YAML；
- 使用其他机场；
- 使用自建节点；
- 使用 FlClash 原生 Profile。

特点：

```text
不依赖 Koyun Platform
不依赖登录
不依赖授权
不依赖任何机场
```

即使控制平台全部故障，本地模式仍然可以使用。

---

## 3.2 托管模式 Managed Mode

用户可以选择某个机场：

```text
选择服务商
   │
   ├── 可云
   ├── 机场 B
   └── 机场 C
```

然后：

```text
登录机场账号
      ↓
获取 Managed Profile
      ↓
自动更新
      ↓
实时推送
      ↓
账号权益
      ↓
设备授权
```

托管模式不是整个 App 的登录系统。

它只是：

```text
App 内的一种 Profile Provider
```

---

# 4. 多机场共存

同一个用户可以同时添加：

```text
Profiles
├── 可云
│   └── Managed Profile
│
├── 机场 B
│   └── Managed Profile
│
├── 机场 C
│   └── Managed Profile
│
├── 自己购买的其他机场
│   └── Local URL Profile
│
└── 自建节点
    └── Local YAML
```

第一阶段可以限制：

```text
一个 Tenant 一个 Managed Profile
```

但数据库和接口必须允许：

```text
一个用户绑定多个 Tenant
```

---

# 5. 多租户系统

后端必须从第一版就支持 Tenant。

Tenant 表：

```text
tenants
```

字段建议：

```text
id
slug
name
status
brand_name
logo_url
icon_url
website_url
support_url
privacy_url
terms_url

panel_type
panel_base_url
panel_config_encrypted

client_access_mode
default_device_limit
offline_grace_seconds

stable_channel
beta_channel

created_at
updated_at
```

例如：

```json
{
  "id": "tenant_koyun",
  "slug": "koyun",
  "name": "可云",
  "panel_type": "xboard",
  "status": "active"
}
```

以后：

```json
{
  "id": "tenant_demo",
  "slug": "demo",
  "name": "Demo VPN",
  "panel_type": "xboard"
}
```

---

# 6. Tenant 与客户端关系

客户端不写死：

```text
api.koyun.xxx
```

而是由 Tenant Configuration 提供：

```json
{
  "tenant_id": "tenant_koyun",
  "name": "可云",
  "api_endpoints": [
    "https://api1.example.com",
    "https://api2.example.com"
  ],
  "ws_endpoint": "wss://push.example.com/ws"
}
```

---

# 7. Tenant Discovery

预留：

```http
GET /v1/public/tenants
```

返回当前客户端允许显示的机场列表。

例如：

```json
[
  {
    "id": "koyun",
    "name": "可云",
    "logo": "...",
    "login_enabled": true
  }
]
```

第一阶段可以只返回：

```text
可云
```

以后授权机场加入后台即可上线。

---

# 8. 白标授权系统

未来需要支持两种授权方式。

---

## 8.1 Shared Client

一个通用客户端：

```text
Koyun Client
```

用户打开后：

```text
选择服务商
```

例如：

```text
可云
机场 B
机场 C
```

优点：

- 只维护一个客户端；
- 更新方便；
- 多机场共存；
- 适合平台化。

---

## 8.2 White Label Client

为某个机场生成独立品牌客户端：

```text
ABC Client
```

品牌配置：

```text
app_name
logo
icon
primary_color
support_url
default_tenant
package_name
windows_product_name
android_application_id
update_channel
```

例如：

```text
default_tenant = tenant_abc
```

客户端打开后直接显示：

```text
ABC 登录
```

但仍建议保留：

```text
本地模式
```

是否显示其他 Tenant，由后台控制。

---

# 9. Brand Profile

建立：

```text
brands
```

字段：

```text
id
tenant_id
name
app_name
short_name
logo_url
icon_url
theme_config
support_url
website_url
privacy_url
terms_url

allow_local_mode
allow_other_tenants
default_tenant_id

windows_app_id
windows_product_name

android_application_id

release_channel
status
```

同一 Tenant 甚至可以有多个 Brand。

---

# 10. License / 授权对象

授权不是：

```text
授权 FlClash 源代码
```

而是：

```text
授权某个 Tenant 使用我们的 Managed Client Platform 服务
```

后台要有：

```text
tenant_licenses
```

字段：

```text
id
tenant_id

status

license_type
start_at
expire_at

max_users
max_active_devices
max_brands

allow_windows
allow_android
allow_linux

allow_realtime
allow_white_label
allow_custom_domain

created_at
updated_at
```

以后可以做不同套餐：

```text
Starter
Professional
Enterprise
```

---

# 11. 前期不收费也要预留

第一阶段：

```text
license_type = internal
```

可云无限制。

其他机场还没有上线。

但所有接口必须已经存在：

```text
TenantLicenseService
```

以后只需要：

```text
写后台
+
开通 License
```

不需要重构客户端。

---

# 12. 租户管理员

不能只有平台超级管理员。

角色：

```text
platform_super_admin
tenant_admin
tenant_operator
tenant_support
```

平台管理员：

```text
管理所有 Tenant
创建机场
授权机场
冻结机场
管理版本
查看全平台状态
```

机场管理员：

```text
只能管理自己的 Tenant
```

例如：

```text
用户
设备
品牌
公告
客户端版本策略
Endpoint
实时推送
```

---

# 13. 数据隔离

所有业务表必须带：

```text
tenant_id
```

例如：

```text
managed_accounts
devices
entitlements
profile_cache
notices
realtime_events
audit_logs
```

数据库查询必须强制 Tenant Scope。

禁止：

```sql
SELECT * FROM devices;
```

业务服务必须使用：

```text
tenant_id
```

进行隔离。

---

# 14. Panel Adapter

这是第二个核心。

定义统一接口：

```rust
trait PanelAdapter {
    async fn authenticate(...);
    async fn get_user(...);
    async fn get_entitlement(...);
    async fn get_subscription(...);
    async fn refresh_subscription(...);
}
```

第一阶段：

```text
XBoardAdapter
```

未来：

```text
V2BoardAdapter
SSPanelAdapter
CustomPanelAdapter
```

客户端不需要知道机场用什么面板。

---

# 15. Adapter 注册

服务端：

```text
panel/
├── mod.rs
├── traits.rs
├── registry.rs
│
├── xboard/
│   ├── auth.rs
│   ├── user.rs
│   └── subscription.rs
│
└── mock/
```

注册：

```text
panel_type = xboard
```

自动加载：

```text
XBoardAdapter
```

---

# 16. 第一阶段 XBoard

可云作为第一个正式 Tenant：

```text
tenant = koyun
panel = xboard
```

第一阶段只需要：

```text
KoyunTenant
+
XBoardAdapter
```

但绝不能把：

```text
XBoard
```

写死在 Controller 里。

---

# 17. Managed Account

用户在某 Tenant 登录后创建：

```text
managed_accounts
```

字段：

```text
id
tenant_id
external_user_id
email_normalized
status
created_at
last_login_at
```

这里：

```text
external_user_id
```

就是 XBoard User ID。

---

# 18. 用户密码

绝对不保存。

流程：

```text
Client
   ↓
Platform API
   ↓
PanelAdapter.authenticate()
   ↓
XBoard
```

成功之后：

```text
Client Platform
```

发自己的：

```text
access_token
refresh_token
```

---

# 19. Profile 类型

统一定义：

```text
ProfileSourceType
```

枚举：

```text
local_url
local_file
local_qr
managed
```

Managed Profile：

```text
provider_type = tenant
tenant_id = xxx
managed_profile_id = xxx
```

---

# 20. Managed Profile

结构：

```text
ManagedProfile
├── id
├── tenantId
├── accountId
├── version
├── etag
├── lastSyncAt
├── lastSuccessAt
├── status
└── LKG
```

---

# 21. 本地 Profile 不受授权影响

这是硬性要求。

任何情况下：

```text
Tenant License 过期
Tenant 被禁用
用户套餐过期
设备超限
Platform API 故障
```

都不得影响：

```text
Local Profile
```

---

# 22. 多机场授权失效行为

例如：

```text
机场 B License 已过期
```

客户端行为：

```text
机场 B Managed Profile
→ 暂停远程同步
→ 显示服务暂不可用
```

但是：

```text
可云 Managed Profile
→ 正常
```

以及：

```text
本地 Profile
→ 正常
```

---

# 23. 实时推送抽象

不能写成：

```text
XBoard WS
```

而要写成：

```text
RealtimeProvider
```

统一事件：

```text
profile.changed
entitlement.changed
account.disabled
device.revoked
notice.published
app.policy.changed
tenant.policy.changed
```

---

# 24. Realtime 接口

服务端：

```text
RealtimeEventBus
```

流程：

```text
XBoard
   ↓
Adapter / Change Detector
   ↓
Normalized Event
   ↓
Redis Pub/Sub
   ↓
WS Gateway
   ↓
Client
```

第一阶段可云可以接现有 XBoard WS。

未来其他机场：

```text
XBoard WS
Webhook
Polling
Custom Event API
```

最后全部转换成标准事件。

---

# 25. 事件格式

统一：

```json
{
  "event_id": "evt_xxx",
  "tenant_id": "koyun",
  "type": "profile.changed",
  "version": 1024,
  "created_at": 1790000000,
  "payload": {}
}
```

客户端必须根据：

```text
tenant_id
```

找到对应 Managed Profile。

---

# 26. WS 只发通知

禁止通过 WS 发完整订阅。

正确：

```text
WS:
profile.changed
```

然后：

```text
HTTPS:
GET /v1/managed/profiles/:id
```

---

# 27. Missed Event Recovery

客户端重连 WS 后：

```text
GET /v1/sync/state
```

服务器返回：

```json
{
  "profile_version": 1050,
  "entitlement_version": 32,
  "policy_version": 8
}
```

客户端比较本地版本。

即使断线期间漏消息，也能恢复。

---

# 28. 三层同步

Managed Profile：

```text
WS 实时事件
+
启动时同步
+
定时兜底同步
```

默认：

```text
启动一次
WS 实时
每 6 小时兜底
```

---

# 29. Last Known Good

每个 Managed Profile 独立 LKG。

例如：

```text
lkg/
├── koyun/
│   └── xxx.yaml
│
└── airport-b/
    └── xxx.yaml
```

Tenant A 更新失败不能影响 Tenant B。

---

# 30. Offline Grace

每个 Tenant 独立：

```text
offline_grace_seconds
```

例如：

```text
可云 = 72h
机场 B = 24h
```

服务端策略下发。

---

# 31. Endpoint 隔离

每个 Tenant 可以有：

```text
API endpoints
WS endpoints
profile endpoints
```

但推荐：

```text
Client
→ Platform Gateway
→ Tenant Panel
```

优先统一平台 API。

---

# 32. Tenant Domain

未来机场可以绑定自己的域名：

```text
client-api.airport-a.com
```

后台映射：

```text
Host
→ tenant_id
```

这就是 White Label API。

---

# 33. Bootstrap

公共客户端：

```http
GET /v1/bootstrap
```

白标客户端：

```text
内置 brand_id
```

启动：

```text
brand_id
   ↓
Bootstrap
   ↓
Tenant
   ↓
Brand config
   ↓
Endpoints
```

---

# 34. Bootstrap 必须签名

配置：

```json
{
  "brand_id": "brand_koyun",
  "version": 10,
  "tenant_id": "koyun",
  "endpoints": [],
  "features": {}
}
```

数字签名。

客户端内置平台公钥。

---

# 35. Feature Flags

从第一阶段预留：

```text
feature_flags
```

例如：

```json
{
  "realtime": true,
  "local_mode": true,
  "multi_tenant": false,
  "managed_profile": true,
  "device_management": true
}
```

以后新功能不需要立即发新客户端界面逻辑。

---

# 36. Client Capability

客户端登录时上报：

```json
{
  "capabilities": [
    "managed_profile_v1",
    "realtime_v1",
    "etag_v1",
    "lkg_v1"
  ]
}
```

服务器根据能力返回兼容策略。

为后续二次升级留接口。

---

# 37. API Versioning

第一版必须：

```text
/v1/
```

以后：

```text
/v2/
```

不要写：

```text
/api/login
```

必须写：

```text
/v1/auth/login
```

---

# 38. Provider API

客户端抽象：

```text
ManagedProvider
```

接口：

```text
login()
logout()
refreshSession()
getAccount()
getEntitlement()
getProfile()
getSyncState()
connectRealtime()
```

当前：

```text
PlatformManagedProvider
```

以后可以增加：

```text
DirectXBoardProvider
CustomProvider
```

但默认统一走 Platform。

---

# 39. 登录 UI

第一次打开：

```text
欢迎使用

[ 添加服务商账号 ]
[ 导入订阅 ]
[ 导入配置文件 ]
```

添加服务商：

```text
选择服务商

可云
机场 B
机场 C
```

---

# 40. 白标客户端 UI

如果：

```text
brand.allow_other_tenants = false
```

那么：

```text
只显示本 Tenant
```

例如：

```text
ABC Client

[ 登录 ABC ]
[ 本地使用 ]
```

---

# 41. 授权给其他机场

平台后台：

```text
创建机场
    ↓
创建 Tenant
    ↓
填写 Panel URL
    ↓
选择 XBoard
    ↓
配置 Adapter
    ↓
设置 License
    ↓
设置品牌
    ↓
生成 API Credential
    ↓
测试
```

然后即可：

```text
Shared Client 上线
```

或：

```text
White Label Client 构建
```

---

# 42. Tenant Onboarding

后台向导：

```text
Step 1 基本信息
Step 2 面板类型
Step 3 面板地址
Step 4 API 验证
Step 5 品牌
Step 6 权限
Step 7 设备策略
Step 8 Realtime
Step 9 测试
Step 10 启用
```

---

# 43. Tenant API Secret

机场后台和 Platform 通讯时使用：

```text
tenant_api_key
```

服务端只存：

```text
hash
```

支持：

```text
rotate
revoke
multiple keys
```

---

# 44. Webhook 接口

必须从第一版预留。

```http
POST /v1/tenant/webhooks/events
```

未来机场可以主动通知：

```text
user.updated
subscription.changed
plan.changed
node.changed
```

---

# 45. Webhook 验证

Header：

```text
X-Tenant-ID
X-Timestamp
X-Signature
```

签名：

```text
HMAC-SHA256
```

防重放：

```text
timestamp
event_id
```

---

# 46. 为什么 Webhook 必须现在预留

有些机场：

```text
没有 WS
```

但是可以：

```text
Webhook
```

有些：

```text
只有 API
```

那就：

```text
Polling
```

Platform 最终都标准化：

```text
RealtimeEvent
```

---

# 47. Tenant Realtime Source

枚举：

```text
NONE
XBOARD_WS
WEBHOOK
POLLING
CUSTOM
```

第一阶段：

```text
Koyun = XBOARD_WS / 现有机制
```

其他机场暂时：

```text
NONE
```

但接口存在。

---

# 48. 管理后台分层

## Platform Admin

管理：

```text
Tenants
Licenses
Brands
Platform releases
Global endpoints
Platform audit
```

## Tenant Admin

管理：

```text
自己的 users
自己的 devices
自己的 notices
自己的 brand
自己的 policy
自己的 endpoints
```

---

# 49. 多机场授权后台

Platform Tenant 列表：

```text
机场
状态
License
用户数
设备数
Windows
Android
Linux
Realtime
White Label
到期时间
```

操作：

```text
启用
暂停
续期
修改额度
重置密钥
创建 Brand
```

---

# 50. License 状态

```text
trial
active
grace
suspended
expired
revoked
```

注意：

```text
Tenant License expired
```

只能影响：

```text
Managed Services
```

不能让通用客户端 Local Mode 失效。

---

# 51. 计费接口预留

前期不做支付。

但表结构预留：

```text
billing_customer_id
billing_plan
billing_cycle
next_billing_at
```

不要实现实际支付。

---

# 52. 使用量计数

以后可以统计：

```text
active users
active devices
managed profiles
realtime connections
```

这些用于授权计费。

禁止统计用户代理访问内容。

---

# 53. Client Build Matrix

公共版本：

```text
brand = default
```

白标：

```text
brand = abc
```

构建命令未来支持：

```bash
dart setup.dart linux --brand abc
dart setup.dart windows --brand abc
dart setup.dart android --brand abc
```

第一阶段可以先不真正实现 `--brand`，但配置层必须支持。

---

# 54. Brand Build Config

目录：

```text
brands/
├── default.json
├── koyun.json
└── sample.json
```

示例：

```json
{
  "brand_id": "koyun",
  "app_name": "可云",
  "default_tenant": "koyun",
  "allow_local_mode": true,
  "allow_other_tenants": true
}
```

---

# 55. 不要 Fork 一份代码给一个机场

严禁：

```text
airport-a-client repo
airport-b-client repo
airport-c-client repo
```

否则以后完全无法维护。

正确：

```text
一个 client codebase
+
多个 brand config
```

---

# 56. Upstream FlClash

Git：

```text
upstream = chen08209/FlClash
origin = 我们自己的 client platform
```

必须继续保持 upstream merge。

---

# 57. Koyun Module 改名

上一版：

```text
lib/koyun/
```

这一版建议改为：

```text
lib/platform/
```

结构：

```text
lib/platform/
├── auth/
├── tenant/
├── provider/
├── managed_profile/
├── realtime/
├── device/
├── bootstrap/
├── release/
└── brand/
```

可云专属：

```text
assets/brands/koyun/
```

---

# 58. 推荐客户端目录

```text
lib/
├── platform/
│   ├── api/
│   ├── auth/
│   ├── tenant/
│   ├── brand/
│   ├── provider/
│   ├── profile/
│   ├── realtime/
│   ├── device/
│   ├── bootstrap/
│   ├── release/
│   └── state/
│
└── FlClash 原代码
```

---

# 59. 服务端项目名称

建议：

```text
koyun-client-platform
```

不是：

```text
koyun-client-control
```

因为未来不仅服务可云。

---

# 60. 服务端目录

```text
koyun-client-platform/
├── api/
├── worker/
├── admin-web/
├── migrations/
├── adapters/
├── realtime/
├── licensing/
├── tenants/
├── brands/
├── deploy/
└── docs/
```

---

# 61. Docker

```text
platform-api
platform-worker
platform-admin
platform-postgres
platform-redis
```

---

# 62. 第一阶段数据库

至少：

```text
tenants
tenant_licenses
brands
managed_accounts
devices
sessions
refresh_tokens
entitlements
managed_profiles
profile_cache
realtime_events
release_channels
releases
notices
admin_users
admin_roles
admin_audit_logs
tenant_api_keys
feature_flags
```

---

# 63. API 第一阶段

公共：

```text
GET /v1/bootstrap
GET /v1/public/tenants
GET /v1/public/brands/:id
```

认证：

```text
POST /v1/auth/login
POST /v1/auth/refresh
POST /v1/auth/logout
```

Tenant：

```text
GET /v1/tenants/:id/me
GET /v1/tenants/:id/entitlement
```

Profile：

```text
GET /v1/managed/profiles
GET /v1/managed/profiles/:id
GET /v1/managed/profiles/:id/state
```

Device：

```text
POST /v1/devices/register
GET /v1/devices
DELETE /v1/devices/:id
```

Realtime：

```text
GET /v1/realtime/token
WS  /v1/realtime/ws
GET /v1/sync/state
```

App：

```text
GET /v1/app/policy
GET /v1/notices
```

---

# 64. Tenant Admin API

```text
GET /v1/admin/tenant
GET /v1/admin/users
GET /v1/admin/devices
GET /v1/admin/notices
GET /v1/admin/brands
GET /v1/admin/policies
GET /v1/admin/endpoints
```

---

# 65. Platform Admin API

```text
GET  /v1/platform/tenants
POST /v1/platform/tenants
PUT  /v1/platform/tenants/:id

GET  /v1/platform/licenses
POST /v1/platform/licenses

GET  /v1/platform/brands
POST /v1/platform/brands

GET  /v1/platform/releases
```

---

# 66. 第一阶段只上线可云

虽然数据库支持：

```text
多 Tenant
```

但 Linux MVP：

```text
Tenant = koyun
```

只需要可云真正连接 XBoard。

其他 Tenant 仅：

```text
Mock Tenant
```

用于测试。

---

# 67. Mock Tenant

必须实现：

```text
tenant_demo
```

Mock：

```text
账号 demo@example.com
密码 demo
```

支持：

```text
login
profile
entitlement
realtime
device
```

用于验证：

```text
代码没有写死可云
```

---

# 68. 多租户验收

第一阶段即使没有真实机场 B，也必须完成：

```text
可云 Tenant
+
Demo Tenant
```

同一客户端可以：

```text
登录可云
登录 Demo
```

并创建两个 Managed Profile。

这才证明架构成立。

---

# 69. Linux MVP

第一阶段：

```text
Local Mode
Tenant System
Brand System
Koyun XBoard Adapter
Demo Adapter
Managed Login
Managed Profile
ETag
LKG
Realtime interface
WS reconnect
Device Authorization
Tenant Licensing
Platform Admin
Tenant Admin
```

---

# 70. 第一阶段 Realtime

如果现有 XBoard WS 接入成本过高：

允许：

```text
接口完成
+
Mock Realtime
+
Polling fallback
```

但必须把：

```text
RealtimeEventBus
WS Gateway
RealtimeProvider trait
```

写出来。

后面直接升级，不重构。

---

# 71. 本地模式验收

必须证明：

```text
Platform API 完全关闭
```

仍能：

```text
打开 App
导入 URL
导入 YAML
启动 mihomo
正常使用
```

---

# 72. Tenant 故障隔离

测试：

```text
Koyun API fail
```

Demo 仍正常。

测试：

```text
Demo fail
```

Koyun 仍正常。

---

# 73. License 故障隔离

测试：

```text
Demo License expired
```

只影响 Demo Managed Profile。

可云与 Local Profile 正常。

---

# 74. 安全

必须：

```text
Tenant secret encryption
Tenant API key hash
Refresh token hash
Tenant scope
RBAC
Admin audit
Rate limiting
Webhook signature
Manifest signature
Release hash
```

---

# 75. Tenant 配置加密

Panel：

```text
admin token
server token
api credential
```

禁止明文存 DB。

使用：

```text
application master key
+
AEAD encryption
```

---

# 76. 审计

Platform 管理操作必须记录：

```text
who
tenant
action
target
timestamp
ip
result
```

---

# 77. 日志隔离

日志必须带：

```text
tenant_id
request_id
```

方便以后多个机场排查。

---

# 78. 版本策略

可以存在：

```text
Global Policy
+
Brand Policy
+
Tenant Policy
```

优先级：

```text
Brand
> Tenant
> Global
```

---

# 79. Release

第一阶段公共 Client：

```text
Koyun Client Platform
```

后面：

```text
White Label
```

可以有独立 Release Channel。

---

# 80. Feature Gate

License 可以控制：

```text
allow_realtime
allow_white_label
allow_custom_domain
allow_multi_brand
allow_custom_update_channel
```

---

# 81. Codex 不要实现的内容

第一阶段不要：

```text
支付
自动开通收费
跨 Tenant 账单
复杂计费
自动生成几十个白标安装包
应用商店发布
完整多语言管理后台
```

只实现基础架构。

---

# 82. Codex 开发顺序

## PR 1

```text
Fork FlClash
建立 upstream
建立 platform module
Local Mode 保持原样
```

## PR 2

```text
Rust Platform API
Postgres
Redis
Tenant
Brand
License
```

## PR 3

```text
PanelAdapter
XBoardAdapter
MockAdapter
```

## PR 4

```text
Managed Account Login
Access Token
Refresh Token
Device
```

## PR 5

```text
Managed Profile
ETag
LKG
Atomic Apply
```

## PR 6

```text
Realtime Event Bus
WS interface
Reconnect
Sync State
```

## PR 7

```text
Platform Admin
Tenant Admin
License
```

## PR 8

```text
Linux build
E2E
Multi Tenant test
```

---

# 83. Codex 代码规则

1. 不允许核心代码写死 `koyun`。
2. 不允许核心代码写死 `xboard`。
3. 所有 Managed 业务必须携带 `tenant_id`。
4. Local Mode 不得依赖 Platform。
5. 一个 Tenant 失败不得影响其他 Tenant。
6. Tenant License 失效不得影响 Local Mode。
7. PanelAdapter 必须 trait/interface。
8. RealtimeProvider 必须 trait/interface。
9. Koyun 只是一条 Tenant 数据。
10. Demo Tenant 必须存在用于架构测试。
11. 不大改 FlClash Core。
12. 不删除 FlClash 原 Profile 功能。
13. 不提交任何生产 Token。
14. 不记录用户密码。
15. 不记录订阅内容。
16. 所有 Profile 更新先 validate。
17. 所有 Managed Profile 有独立 LKG。
18. 所有 WS 重连后做 Sync State 校验。
19. API 使用 `/v1`。
20. 所有数据库修改使用 migration。

---

# 84. Definition of Done

Linux 第一阶段必须：

```text
[ ] FlClash Local Mode 完整可用
[ ] 不登录任何账号也可以使用
[ ] 可以导入任意兼容订阅
[ ] 可以导入 YAML

[ ] Tenant 模型完成
[ ] Brand 模型完成
[ ] Tenant License 模型完成

[ ] Koyun Tenant 可登录真实 XBoard
[ ] Demo Tenant 可登录 Mock Adapter

[ ] 同一客户端可绑定多个 Tenant
[ ] 每个 Tenant 有独立 Managed Profile

[ ] Managed Profile 自动同步
[ ] ETag 正常
[ ] LKG 正常
[ ] Profile 错误自动回滚

[ ] RealtimeProvider 接口完成
[ ] WS Client 接口完成
[ ] WS 重连完成
[ ] Sync State 完成
[ ] Polling fallback 完成

[ ] Device Authorization 完成
[ ] Tenant Device Policy 完成

[ ] Tenant License 能启用/暂停
[ ] License 失效只影响对应 Tenant

[ ] Platform Admin 可创建 Tenant
[ ] Platform Admin 可授权 Tenant
[ ] Tenant Admin 只能管理自己 Tenant

[ ] Platform API 故障不影响 Local Mode
[ ] Tenant A 故障不影响 Tenant B

[ ] flutter analyze 通过
[ ] flutter test 通过
[ ] Rust tests 通过
[ ] Linux Release Build 通过

[ ] upstream merge 文档完成
[ ] GPL attribution 完成
```

---

# 85. 最终商业模式结构

最终平台：

```text
                 Koyun Client Platform
                          │
             ┌────────────┴─────────────┐
             │                          │
        Shared Client               White Label
             │                          │
      ┌──────┼──────┐                Airport A
      │      │      │                Branded App
    可云   机场B   机场C
      │      │      │
   XBoard  XBoard  Other Panel
```

同时所有客户端永远保留：

```text
Local Mode
```

---

# 86. 项目商业边界

我们提供给其他机场的不是：

```text
“卖一份改过的 FlClash”
```

而是：

```text
Client Platform Service
+
Tenant Authorization
+
Managed Login
+
Managed Profile
+
Device System
+
Realtime
+
White Label
+
Release Management
```

这才是长期可以运营的产品。

---

# 87. 最终项目原则

整个系统从第一天就必须满足：

```text
可云只是第一个机场
XBoard 只是第一个 Panel
Linux 只是第一个平台
Shared Client 只是第一种发行方式
Realtime 只是可插拔能力
```

因此核心架构一定是：

```text
Multi Tenant
Multi Brand
Multi Provider
Multi Platform
Dual Mode
```

不要为了第一阶段省一点代码，把未来升级空间写死。

---

# 88. Codex 最终开工指令

请 Codex：

```text
1. 以 chen08209/FlClash 为 upstream。
2. 不删除 FlClash 原有本地 Profile 能力。
3. 将新增模块命名为 Platform，而不是 Koyun。
4. 设计 Tenant / Brand / License。
5. 创建 PanelAdapter trait。
6. 实现 XBoardAdapter。
7. 创建 MockAdapter 验证非 XBoard 场景。
8. 实现 Managed Account。
9. 实现 Managed Profile。
10. 实现每 Tenant 独立 LKG。
11. 实现 Device Authorization。
12. 实现 RealtimeProvider trait。
13. 实现统一 Event Bus。
14. 实现 WS Client 接口。
15. 实现重连和 Sync State。
16. 实现 Polling fallback。
17. 实现 Platform Admin。
18. 实现 Tenant Admin。
19. 实现 Tenant License。
20. 完成 Linux E2E。
21. 用 Koyun + Demo 两个 Tenant 做多租户验收。
22. Linux 未通过完整验收前，不做 Windows / Android。
```

---

## 项目名称建议

代码层推荐：

```text
Client:
koyun-client

Platform:
koyun-client-platform
```

对外商业名称以后可以再改。

架构内部不要把核心能力写死成：

```text
Koyun only
```

必须始终按照：

```text
Client Platform
```

开发。
