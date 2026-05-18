# PX 快速开始

这份文档只回答一件事：

怎样最快把本地链路跑通。

## 1. 生成开发证书

在项目根目录执行：

```bash
scripts/generate-dev-cert.sh
```

默认会生成：

- `config/server-cert.pem`
- `config/server-key.pem`
- 自签 TLS 证书会带 `SAN` 和 `serverAuth`，用于客户端按证书文件固定服务端身份
- 这组证书只用于本地联调，不用于 VPS 生产目录

## 2. 启动服务端

```bash
scripts/start-server-dev.sh
```

默认读取 `config/server.toml`。

## 3. 启动 PX 个人代理 GUI 开发模式

```bash
scripts/install-dev-px-tun-helper.sh
scripts/start-gui-dev.sh
```

启动后：

- GUI 会读取当前运行目录下的 `config/client.toml`
- GUI 会启动共享 runtime
- GUI 开发态默认会使用 `bin/px-tun-helper`
- GUI 默认保持 `SOCKS5 -> runtime` 主路径，同时预留本地 ingress 供 TUN helper 直连
- GUI 会使用 `config/server-cert.pem` 连接服务端
- 若服务端重签证书，必须把新的 `server-cert.pem` 重新导入客户端

## 4. 做一次冒烟测试

另开一个终端执行：

```bash
scripts/smoke-test.sh
```

默认会通过本地 SOCKS5 访问 `https://example.com`。

## 5. 如需启用 TUN

macOS / Linux:

```bash
scripts/install-dev-px-tun-helper.sh
```

Windows PowerShell:

```powershell
./scripts/repair-tun-helper.ps1
```

开发态说明：

- 当前开发态默认 helper 已切到 Go `px-tun-helper`
- `scripts/install-dev-px-tun-helper.sh` 会本地编译 helper，并安装到 `apps/tauri-ui/.px-dev-runtime/bin/px-tun-helper`
- 开发态 TUN 默认走 `helper -> ingress -> runtime`

补充说明：

- macOS 开发态若要启用 TUN，还需先执行 `cargo build -p px-dns-helper`
- Windows 修复脚本默认优先使用当前仓库的缓存 Release：`tun-helper-cache-v1`，失败后再回退官方源
- macOS 正式发布包默认会自带 `bin/px-tun-helper`
- 如果正式发布后的 `bin/px-tun-helper` 被手动删掉，建议直接重新解压发布包

## 6. 开发态 ingress 联调

当前开发态默认链路是：

- Go `px-tun-helper`
  -> 本地 ingress listener
  -> px-runtime

直接执行：

```bash
scripts/install-dev-px-tun-helper.sh
scripts/start-gui-dev.sh
```

当前开发态默认不会再走旧的本地 `SOCKS5 bridge` 过渡链，而是直接：

```text
px-tun-helper
  -> 本地 ingress listener
  -> px-runtime
  -> TLS 加密隧道
  -> px-server
```

说明：

- 这是当前开发态默认联调路径
- 开发态已经实测可跑通 `baidu`、`google`、`github`
- 正式发布 macOS 默认路径也已切到 Go `px-tun-helper`
- 当前主要剩余问题是少量冷连接首访偏慢，不是链路不可用

最小验证顺序：

1. 启动 GUI 开发态
2. 点击“启动客户端”
3. 点击“连通性测试”
4. 点击“开启 TUN”
5. 用浏览器验证 `google` 等站点
6. 停止 TUN 后确认 DNS 恢复

## 结果判断

如果一切正常，你应该能看到：

- 服务端正常启动
- GUI 能显示运行中
- 冒烟测试通过

## 额外说明

- 正式客户端只有 GUI：`PX 个人代理`
- 首版只支持 TCP，不支持 UDP
- 开发态 TUN 当前默认通过 Go `px-tun-helper` 接到本地 ingress，目前只做全局 TCP
- macOS TUN 模式会额外启动本地 `DNS helper`，把当前主网卡 DNS 临时切到 `127.0.0.1`；开发态默认经 ingress 转发解析请求
- 因为当前仍是 `TCP-only`，浏览器里的 QUIC / HTTP3 / STUN 一类 UDP 流量不会被代理
- 当前客户端按 `server_cert_path` 固定服务端证书文件
- 本地建议先用 `127.0.0.1` 联调，跑通后再换成 Rocky9 VPS

更多文档：

- Rocky9 部署：[rocky9-deploy.md](file:///Users/qiufeihai/workspace/px/docs/rocky9-deploy.md)
- GUI 打包：[client-packaging.md](file:///Users/qiufeihai/workspace/px/docs/client-packaging.md)
