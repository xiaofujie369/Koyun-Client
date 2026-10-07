# 实现计划与验收记录

## 当前状态（2026-10-07）

已读取并归档 V3 需求。工作区初始为空，现已获取 FlClash 完整 Git 历史并创建
`codex/platform-linux-mvp` 分支。尚未修改原客户端业务代码，尚未实现客户端真实 XBoard 联动。
Rust 租户范围、License、离线宽限及 PanelAdapter 共 28 项测试通过，fmt 与 Clippy 通过。
XBoardAdapter 已在 VPS 使用独立测试账号完成真实登录、稳定 ID 校验、权益和配置拉取。
实际面板 user/info 已补充本人稳定数字 ID，原文件有私密备份，PHP 语法及真实接口验证通过。
首个 GitHub Actions 工作流 37574552728 已通过：后端检查、Flutter analyze、Flutter test、
Linux Release bundle 和开发产物上传。该产物仍是上游基础界面，并非完整托管客户端。
公网入口仍需验证，平台 API、数据库、客户端托管界面与完整 E2E 尚未完成。

本机 Flutter 3.38.3 / Dart 3.10.1；上游说明要求 Flutter 3.47.x，发布 CI 固定 3.47.4。
Windows PATH 中未找到 cargo、rustc、docker。已发现现有 WSL 发行版，但未修改其配置或服务。
已在本机 WSL /opt/koyun-dev 安装隔离 Rust 工具链，未修改已有 PATH。
用户已提供现有 VPS；资源较紧张且有现存业务，后端联调与客户端 CI 编译分开。

## 架构决策

客户端保留 FlClash 的本地配置与 Core 生命周期。新增 `lib/platform/` 持有租户、账户、
托管配置、设备、同步与品牌逻辑。平台网络失败不阻塞客户端启动或本地模式。

后端使用 Rust，分离 HTTP 层、领域服务与 PanelAdapter。PostgreSQL 保存业务事实，
Redis 用于限流与通知分发，不作为授权和同步版本的唯一事实来源。
第一阶段仅配置一个真实租户和一个开发 Demo 租户；生产不得自动开启 Demo 账号。

一租户一托管配置是首版产品限制，数据标识使用 tenant_id + account_id + profile_id。
同一客户端可持有多个独立租户会话；租户切换不复用 access token。
服务端从经过认证的会话派生租户范围，拒绝客户端路径或请求头与会话范围不一致的请求。
跨表引用使用包含 tenant_id 的复合外键，避免只靠 Controller 过滤。

## 需要补齐的协议细节

1. API Key 只存哈希用于认证；Webhook HMAC 使用独立随机签名密钥，经 AEAD 加密保存。
   不能用不可逆的 API Key 哈希还原 HMAC 密钥。验签覆盖原始请求体、时间戳与租户 ID；
   用数据库唯一约束防止同租户 event_id 重放，不能仅靠 Redis Pub/Sub。
2. Bootstrap 使用 Ed25519 签名；签名对象为明确编码的原始 payload 字节，携带 key_id、
   版本、签发与过期时间。客户端固定可信根公钥并拒绝回滚，公钥轮换需要签名链。
3. Refresh token 随机生成、仅保存哈希、一次性轮换；重复使用已轮换 token 撤销会话族。
   XBoard 用户会话凭据如需后续拉取订阅，必须加密存储，密码不落盘、不进入日志。
4. 平台失联可使用每租户独立的 LKG 与离线宽限；收到明确撤销/禁用后立即执行策略，
   不能伪装成网络错误进入宽限。任何处理均不改变 Local Profile。
5. WS 仅推送失效通知，配置走 HTTPS；重连先查询 sync/state，再按版本补拉。
   Redis 丢失通知时，数据库版本与启动/定时同步保证恢复。
6. YAML 先做大小限制、语法检查和 mihomo 兼容验证，再原子替换；应用失败恢复 LKG。
   LKG 路径使用不透明内部 ID，不把可控 tenant slug 直接拼入文件系统路径。
7. License 控制平台服务与品牌能力，不能关闭本地模式。文档中的 allow_local_mode
   在第一阶段固定为 true，服从“始终保留本地模式”的验收要求。
8. API 面板地址与订阅下载地址需限制 HTTPS、重定向及出站目标；不能将任意用户 URL
   当作服务器端订阅拉取目标。特殊私网面板需管理员明确配置允许范围。

## 开发阶段

| 阶段 | 交付 | 验收 |
|---|---|---|
| 1 | 上游源码基线、品牌配置、Platform 模块入口 | 原有本地导入行为不变 |
| 2 | Rust API、迁移、Tenant/Brand/License | 租户隔离、授权边界测试 |
| 3 | PanelAdapter、Mock、XBoard | 两租户登录与错误隔离 |
| 4 | 会话与设备授权 | token 轮换、并发设备额度、撤销 |
| 5 | 托管配置同步、ETag、LKG | 无效 YAML、应用失败、断电恢复 |
| 6 | EventBus、WS、重连与轮询 | 漏消息恢复、租户事件隔离 |
| 7 | 平台与租户后台、审计 | 角色越权及租户越权测试 |
| 8 | Linux 构建、真实面板联调、E2E | 完整执行 V3 Definition of Done |

任何阶段的接口骨架、Mock 演示或编译通过，均不代表 Linux MVP 已完成。
支付、商店发布、Windows/Android 发布与批量白标构建不属于当前阶段。

## 外部依赖

- 已取得实际面板源码信息、普通测试账号并验证稳定用户 ID；公网 API 访问策略仍待解决。
- 已取得部署主机；平台 API 的独立域名与 TLS 接入尚待配置。
- origin 已配置为用户指定的 xiaofujie369/Koyun-Client，保留 FlClash 历史。
- 真实 Linux 桌面上的本地导入、Core、TUN 与断网 E2E 验证。
