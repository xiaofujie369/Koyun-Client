# XBoard 适配边界

当前支持 `/api/v1/passport/auth/login`、`/api/v1/user/info`、
`/api/v1/user/getSubscribe` 和 `/api/v1/client/subscribe?flag=clash.meta`。
`auth_data` 按面板返回的 Bearer 格式原样使用。

`user/info` 必须返回稳定、非零的数字 `id`。部分 XBoard 分支未选取该字段，
可先审查本目录的 `user-info-id.patch`，在对应版本上检查补丁适用性后部署。
补丁仅在原有已认证的用户本人查询中增加 id，不改变权限或登录流程。
不支持该契约时返回 StableIdentityUnavailable，不用可重置 UUID 或邮箱替代永久 ID。

服务器管理员配置面板 HTTPS origin，可选固定 origin IP，保持域名 TLS 校验。
面板地址属于可信部署配置，不能允许普通用户自填后发起服务端请求。
订阅始终从已配置面板的固定路径拉取，忽略响应中任意 subscribe_url，禁止 HTTP 重定向。
个别面板的订阅路径不兼容时应扩展显式配置并测试，不自动跟随第三方 URL。

ProfileDocument 完成有限大小、YAML 解析与基本结构检查；客户端应用前还必须调用
mihomo 的配置验证，不能把 YAML 解析通过等同于可运行配置。

MockAdapter 只应在显式开启的开发租户注册，不能自动在生产启用 Demo 凭据。
