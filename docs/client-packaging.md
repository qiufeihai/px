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
- macOS 本地验证建议直接打 `.app`，避免额外 `dmg` 打包失败干扰 GUI 联调
- Windows 发布包直接发便携目录 `zip`，不依赖 `.msi`

## 2. GitHub Actions 打包

工作流文件：

- [client-release.yml](file:///Users/qiufeihai/workspace/px/.github/workflows/client-release.yml)

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
- 自动上传到 GitHub Release

说明：

- Release 不会包含服务端私钥
- 客户端仍需单独下载服务端生成的 `server-cert.pem`
- Release 会按固定版本自动拉取 `tun2socks`；Windows 还会自动拉取 `wintun.dll`
- 客户端首次使用前仍需按实际 VPS 地址生成自己的 `client.toml`

## 3. 运行时目录约定

当前发布包按“当前运行目录”组织：

```text
px-personal-proxy-xxx/
  bin/
    tun2socks | tun2socks.exe
    wintun.dll
  config/
    client.toml
  gui/
    PX 个人代理.app | PX 个人代理.exe
  scripts/
    create-client-prod-config.(sh|ps1)
    fetch-server-cert.(sh|ps1)
    fetch-tun-helper.(sh|ps1)
    start-gui.(sh|ps1|bat)
```

约定：

- Tauri 控制面也从当前运行目录下的 `config/client.toml` 读取配置
- Tauri 会启动共享 runtime
- 若启用 TUN，GUI 会从当前运行目录下的 `bin/` 查找 helper
- GUI 建议通过 `scripts/start-gui.(sh|ps1|bat)` 从发布目录启动
- 因此最终使用时，建议在解压后的 `PX 个人代理` 发布目录内运行
- 正式发布包默认已自带 `bin/` 下的 helper，一般不需要用户手动下载

macOS 示例：

```bash
cd px-personal-proxy-macos
./scripts/start-gui.sh
```

Windows PowerShell 示例：

```powershell
Set-Location px-personal-proxy-windows
.\scripts\start-gui.ps1
```

## 4. PX 个人代理分发最小集合

- Tauri 打包产物
- `gui/PX 个人代理` 或 `gui/PX 个人代理.exe`
- Windows Release 为便携目录 `zip`，解压后直接运行 `gui/PX 个人代理.exe`
- `config/client.toml`
- `scripts/create-client-prod-config.(sh|ps1)`
- `scripts/fetch-server-cert.(sh|ps1)`
- `scripts/fetch-tun-helper.(sh|ps1)`
- `scripts/start-gui.(sh|ps1|bat)`
- 发布包默认包含 `bin/tun2socks` 或 `bin/tun2socks.exe`
- Windows 发布包默认包含 `bin/wintun.dll`

## 5. 当前注意事项

- `client.toml` 里的 `server_cert_path` 现在可直接写相对路径 `config/server-cert.pem`
- `server_addr` 必须写成实际 VPS 公网 IP 和端口
- `server-cert.pem` 必须和服务端正在使用的证书一致
- 若证书重签发，客户端也必须同步替换证书文件
- Windows 下建议用 PowerShell 脚本生成配置和拉取证书
- 若本地开发目录缺少 helper，可执行 `scripts/fetch-tun-helper.sh` 或 `scripts/fetch-tun-helper.ps1`
- `fetch-tun-helper` 现在默认优先使用当前仓库的缓存 Release：`tun-helper-cache-v1`，失败后再回退官方源
- 正式发布后若用户手动删掉了 `bin/`，也可以直接在 GUI 里点击“下载 helper”
- macOS/Windows GUI 建议优先通过启动脚本启动，以确保工作目录落在发布目录
