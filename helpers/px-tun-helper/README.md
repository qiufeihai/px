# `px-tun-helper`

当前最小骨架分两步：

- 打开 macOS `utun`
- 初始化替代用的 userspace TCP/IP 栈
- 把 TUN 收到的 IPv4 包送进栈入口
- 把栈里出现的新 TCP 连接桥接到现有 ingress
- 把从栈侧生成的回包继续写回 TUN

当前刻意不做：

- GUI 接线
- DNS / 路由 / 提权逻辑
- UDP / IPv6
- 域名保真

当前新增参数：

- `--ingress`
- `--connect-timeout-ms`

开发态安装：

```bash
scripts/install-dev-px-tun-helper.sh
scripts/start-gui-dev.sh
```

这个脚本会：

- 本地编译 `helpers/px-tun-helper`
- 安装到 `apps/tauri-ui/.px-dev-runtime/bin/px-tun-helper`
- 把开发态 `client.toml` 的 `helper_path` 切到 `bin/px-tun-helper`
- 保持 GUI 默认 `SOCKS5 -> runtime` 主路径，同时预留本地 ingress 供 TUN helper 直连

当前联调状态：

- 开发态已可直接复用现有 GUI 和 `scripts/macos-tun-helper.sh`
- 已实测跑通真实浏览器流量，`baidu`、`google`、`github` 可正常访问
- 当前仍可观察到少量首访冷连接偏慢，主要体现在个别 `accept_to_ingress_ok_ms` 会升到约 `1s~3s`

日志策略：

- `info` 默认保留：启动/停止、包计数、错误
- 每条成功流的 `tcp flow accepted`、`ingress target connected`、`tcp bridge finished` 已降到 `debug`
- 需要继续看单流耗时时，可用 `--loglevel debug`
- macOS 正式发布默认 helper 也已切到 Go `px-tun-helper`

当前目标已经从“验证替代 helper”切到“把 Go `px-tun-helper` 作为 macOS 默认 TUN helper 使用”。
