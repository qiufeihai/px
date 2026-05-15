# PX 个人代理

一个只给自己使用的高性能代理项目。

目标很明确：

- 性能优先，尽量无损接近直连
- 低延迟、低抖动、高吞吐、高稳定
- 只做个人场景，不做多用户控制面
- 客户端正式产品只保留 GUI：`PX 个人代理`

## 当前方案

- 服务端：Rust
- 客户端：Tauri 中文 GUI
- 传输：`TCP + TLS 1.3`
- 首版范围：只支持 TCP，不支持 UDP；TUN 通过外部 helper 接到本地 SOCKS5

当前链路：

```text
浏览器 / 应用
  -> 可选 TUN helper
  -> 本地 SOCKS5
  -> PX 共享 runtime
  -> TLS 加密隧道
  -> px-server
  -> 目标 TCP 服务
```

## 为什么这样做

这个项目不是通用平台代理，也不是大而全代理框架。

这里优先追求的是：

- 更短的数据路径
- 更少的兼容层
- 更少的可变策略
- 更稳定的时延表现

所以当前明确不做：

- 多用户
- UDP
- 规则系统
- 大而全协议兼容层

## 最快开始

本地联调：

1. 生成开发证书：`scripts/generate-dev-cert.sh`
2. 启动服务端：`scripts/start-server-dev.sh`
3. 启动客户端 GUI 开发模式：`scripts/start-gui-dev.sh`

4. 冒烟测试：`scripts/smoke-test.sh`

## 生产使用

正式使用路径只有一条：

- 服务端部署到 Rocky Linux 9
- 客户端使用 `PX 个人代理` GUI

推荐流程：

1. VPS 上一键部署服务端
2. 如需单独重签生产证书：`deploy/generate-vps-cert.sh`
3. 手动拷贝服务端公钥证书
4. 在 GUI 里填写客户端配置并导入证书
5. 启动 `PX 个人代理`

## 目录说明

- `apps/tauri-ui`：GUI 前端与 Tauri 后端
- `crates/px-runtime`：共享 runtime
- `crates/px-server`：服务端
- `crates/px-proto`：协议与配置定义
- `crates/px-bench`：性能测试入口
- `config`：示例配置与本地配置
- `scripts`：证书、打包、启动、发布辅助脚本
- `deploy`：VPS 部署与 systemd 文件
- `docs`：详细文档

## 文档入口

- 快速开始：[quickstart.md](file:///Users/qiufeihai/workspace/px/docs/quickstart.md)
- Rocky9 部署：[rocky9-deploy.md](file:///Users/qiufeihai/workspace/px/docs/rocky9-deploy.md)
- GUI 打包与发布：[client-packaging.md](file:///Users/qiufeihai/workspace/px/docs/client-packaging.md)，workflow: [px-release.yml](file:///Users/qiufeihai/workspace/px/.github/workflows/px-release.yml)
- 私有协议说明：[protocol.md](file:///Users/qiufeihai/workspace/px/docs/protocol.md)
- AI 开发规则：[AGENTS.md](file:///Users/qiufeihai/workspace/px/AGENTS.md)

## 当前边界

- 首版只支持 TCP
- 不支持 UDP
- TUN 只支持外部 helper + 全局 TCP
- 不做 Windows Service / macOS 后台托管
- 不做自动分发配置与证书

如果后续要扩功能，默认先问一个问题：

这项改动是否真的能在不损失性能和稳定性的前提下带来明确价值？
