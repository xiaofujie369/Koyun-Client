# Koyun Client Platform

本仓库基于 FlClash，保留其 Git 历史、GPL-3.0 许可证及原生本地订阅功能。
新增功能仍在开发中，目前不能作为已完成的客户端平台发布。

- `platform/`：Rust 领域层与租户授权测试。
- `docs/platform/`：V3 需求、实现计划、部署与上游维护文档。
- `.github/workflows/platform-linux.yaml`：后端测试及 Linux 分析、测试、Release 构建。

首版范围为 Linux、本地模式、多租户托管账户、XBoard Adapter、Demo Adapter、
设备授权、订阅同步与 LKG、实时事件接口和管理后台。

CI 产物标记为 development；基础客户端构建成功不代表托管功能已实现。
生产凭据、真实订阅、服务器私密资料不得上传本仓库或 GitHub Actions 日志。
