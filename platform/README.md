# Platform 后端

当前交付为 Rust 领域层，不是完整 HTTP 服务。

- AccountScope：校验资源同时属于当前租户与账户。
- TenantLicenseService：租户、时间、平台、功能的显式授权检查。
- cached_profile_decision：区分网络失败与明确撤销，限定 LKG 离线宽限。

这些接口只用于 Managed 服务，不接入 Local Profile。
HTTP 身份认证、数据库持久化、真实适配器、管理员界面和部署仍待实现。

```sh
cargo test --manifest-path platform/Cargo.toml
```

本机专用 WSL 工具链位于 XboardAuditValidation 的 /opt/koyun-dev，未修改现有 PATH。
构建缓存也写入 /opt/koyun-dev/target，不占用 VPS。
