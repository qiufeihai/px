# PX 个人代理打包说明

## 目标

- macOS 和 Windows 正式产品统一为 Tauri 中文 GUI `PX 个人代理`

## 1. 构建 Tauri GUI

进入前端目录：

```bash
cd apps/tauri-ui
npm install
npm run tauri build -- --bundles app
```

说明：

- GUI 不承载代理数据面，只负责配置和运行时控制
- 真正转发流量的是共享 runtime
- TUN 只通过外部 helper 接到本地 SOCKS5，不进入 runtime 热路径
- 开发模式下，GUI 会把配置、证书、helper 和日志写到 `apps/tauri-ui/.px-dev-runtime/`，避免 `tauri dev` 因源码目录文件变化而重编译
- macOS 本地验证建议直接打 `.app`，避免额外 `dmg` 打包失败干扰 GUI 联调
- Windows 发布包直接发便携目录 `zip`，不依赖 `.msi`

## 2. GitHub Actions 打包

工作流入口：

- 展示名：`px-release`
- 文件： [px-release.yml](file:///Users/qiufeihai/workspace/px/.github/workflows/px-release.yml)

触发方式：

- 推送 tag，例如 `v0.1.0`
- 或在 GitHub Actions 页面手动 `workflow_dispatch`

工作流会：

- 在 macOS / Windows runner 上构建 `PX 个人代理`
- macOS 构建 `.app`
- Windows 只编译 GUI `.exe`，再组装成便携目录 `zip`
- 构建前自动下载固定版本的 TUN helper
- 若 helper 下载失败或未进入发布目录，构建会直接失败
- 把 `config/client.toml` 示例、辅助脚本、`bin/` helper 和 GUI bundle 一起归档
- 最终归档文件名固定为 `px-${tag}-${os}`，例如 `px-v0.1.0-macos.tar.gz`、`px-v0.1.0-windows.zip`
- 自动上传到 GitHub Release

说明：

- Release 不会包含服务端私钥
- 客户端仍需单独下载服务端生成的 `server-cert.pem`
- Release 会按固定版本自动拉取 `tun2socks`；Windows 还会自动拉取 `wintun.dll`
- 客户端首次使用前仍需按实际 VPS 地址生成自己的 `client.toml`

## 3. 运行时目录约定

当前发布包按“当前运行目录”组织：

```text
px-vX.Y.Z-xxx/
  px.app | px.exe
  bin/
    tun2socks | tun2socks.exe
    wintun.dll
  config/
    client.toml
  scripts/
    create-client-prod-config.(sh|ps1)
    fetch-tun-helper.(sh|ps1)
```

其中：

- `${tag}` 是发布 tag，例如 `v0.1.0`
- `${os}` 目前是 `macos` 或 `windows`

约定：

- Tauri 控制面也从当前运行目录下的 `config/client.toml` 读取配置
- Tauri 会启动共享 runtime
- 若启用 TUN，GUI 会从当前运行目录下的 `bin/` 查找 helper
- Windows 发布包支持直接双击根目录下的 `px.exe`
- 正式发布包默认已自带 `bin/` 下的 helper，一般不需要用户手动下载

macOS 示例：

```bash
cd px-v0.1.0-macos
./scripts/open-macos-app.sh
```

说明：

- macOS 下载包未签名/未公证时，右键打开也可能失败
- `scripts/open-macos-app.sh` 会先对当前发布目录递归移除 `com.apple.quarantine`，再打开 `px.app`
- 每个新下载版本首次使用时，建议先执行一次这个脚本

Windows PowerShell 示例：

```powershell
Set-Location px-v0.1.0-windows
.\px.exe
```

## 4. PX 个人代理分发最小集合

- Tauri 打包产物
- `px.app` 或 `px.exe`
- Windows Release 为便携目录 `zip`，解压后可直接运行根目录下的 `px.exe`
- `config/client.toml`
- `scripts/create-client-prod-config.(sh|ps1)`
- `scripts/fetch-tun-helper.(sh|ps1)`
- macOS 发布包额外包含 `scripts/open-macos-app.sh`
- 发布包默认包含 `bin/tun2socks` 或 `bin/tun2socks.exe`
- Windows 发布包默认包含 `bin/wintun.dll`

## 5. 当前注意事项

- `client.toml` 里的 `server_cert_path` 现在可直接写相对路径 `config/server-cert.pem`
- `server_addr` 必须写成实际 VPS 公网 IP 和端口
- `server-cert.pem` 必须和服务端正在使用的证书一致
- 若证书重签发，客户端也必须同步替换证书文件
- Windows 下建议用 PowerShell 脚本生成配置
- 若本地开发目录缺少 helper，可执行 `scripts/fetch-tun-helper.sh` 或 `scripts/fetch-tun-helper.ps1`
- `fetch-tun-helper` 现在默认优先使用当前仓库的缓存 Release：`tun-helper-cache-v1`，失败后再回退官方源
- 正式发布后若用户手动删掉了 `bin/`，也可以直接在 GUI 里点击“下载 helper”
- GUI 现在会按可执行文件位置定位发布目录，双击根目录下的 `.app` 或 `.exe` 即可运行
