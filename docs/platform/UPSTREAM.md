# FlClash 上游维护

来源：https://github.com/chen08209/FlClash

初始化基线：`4b59eca853778d4e7be3de26589252889c899bc5`。
工作目录：`client/`，远程名：`upstream`，分支：`codex/platform-linux-mvp`。
这是保留历史的本地克隆。origin 指向用户指定的 https://github.com/xiaofujie369/Koyun-Client。
后端位于 platform/，设计记录位于 docs/platform/，与客户端在同一个仓库维护。

后续重建工作副本：

```sh
git clone --origin upstream https://github.com/chen08209/FlClash.git client
git -C client switch -c codex/platform-linux-mvp 4b59eca853778d4e7be3de26589252889c899bc5
```

新增平台代码集中在 lib/platform，品牌放配置目录。不要把业务逻辑复制成多个机场分支。
升级前保存当前工作，fetch upstream，在单独升级分支 merge 目标 tag/commit；处理冲突后
执行 flutter analyze、相关 flutter test、Linux Release build 与本地模式/托管模式回归。

上游 core/Clash.Meta 使用 SSH submodule URL；没有 GitHub SSH 凭据时可仅在本地 Git
配置中覆盖为对应 HTTPS URL，再初始化，避免不必要修改上游 .gitmodules。

保留 client/LICENSE、版权声明及依赖许可证。发布修改客户端时随版本准备对应源代码、
构建说明与修改记录。平台服务授权与客户端上游许可证是不同边界。
许可证原文：https://github.com/chen08209/FlClash/blob/main/LICENSE
