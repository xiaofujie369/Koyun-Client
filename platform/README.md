# Platform 后端

当前交付为 Rust 领域层与面板适配层，不是完整 HTTP 服务。

- AccountScope：校验资源同时属于当前租户与账户。
- TenantLicenseService：租户、时间、平台、功能的显式授权检查。
- cached_profile_decision：区分网络失败与明确撤销，限定 LKG 离线宽限。
- PanelAdapter / AdapterRegistry：按租户分派面板调用。
- XBoardAdapter：真实登录、稳定账户身份、权益查询和固定路径订阅下载。
- MockAdapter：显式注册的 Demo 租户；仅用于开发验证。
- ProfileDocument：大小限制、YAML 基本结构检查与内容 ETag。
- MasterKey / token：AEAD 上下文绑定加密、分用途随机令牌与哈希。
- migrations：20 张业务/管理表、租户 RLS、刷新令牌轮换与重放撤销事务。

这些接口只用于 Managed 服务，不接入 Local Profile。
平台 HTTP 身份认证、数据库访问服务、管理员界面和服务部署仍待实现。
客户端应用配置之前还必须做 mihomo 验证与 LKG 原子替换。

```sh
cargo test --manifest-path platform/Cargo.toml
cargo clippy --manifest-path platform/Cargo.toml --all-targets -- -D warnings
```

真实面板的独立检查工具为 `examples/verify_xboard.rs`，需要私密测试账号 JSON 文件
和显式的源站 IP:port。凭据只在进程内读取，不在输出中展示订阅或 token。
面板兼容性契约见 `adapters/xboard/README.md`。
数据库测试见 `migrations/README.md`。表迁移与事务函数通过不代表完整身份系统已验收。

本机专用 WSL 工具链位于 XboardAuditValidation 的 /opt/koyun-dev，未修改现有 PATH。
构建缓存也写入 /opt/koyun-dev/target，不占用 VPS。
