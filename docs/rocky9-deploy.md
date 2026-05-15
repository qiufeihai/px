# PX Rocky9 部署说明

这份文档只保留生产环境最短路径。

目标：

- 服务端部署到 Rocky Linux 9
- 客户端使用 `PX 个人代理`
- 保持首版极简链路：`SOCKS5 -> TLS -> 目标 TCP`

## 1. VPS 一键部署

在 Rocky9 VPS 上执行：

```bash
git clone <你的仓库地址> /opt/px-src
cd /opt/px-src
sudo SERVER_IP=你的VPS公网IP ./deploy/bootstrap-vps.sh
```

这一步会完成：

- 安装依赖
- 构建 `px-server`
- 生成服务端证书
- 写入 `/opt/px/config/server.toml`
- 安装并启动 `systemd`

说明：

- 默认生成的自签证书会带服务器 IP 的 `SAN` 和 `CA:TRUE`
- 如果后续手动重签证书，必须整对替换 `server-cert.pem` 和 `server-key.pem`
- 证书重签后，客户端也要重新导入新的 `server-cert.pem`

后续更新：

```bash
cd /opt/px-src
sudo ./deploy/update-vps.sh
```

## 2. 检查服务端状态

```bash
sudo systemctl status px
sudo journalctl -u px -f
```

如果 Rocky9 开启了 `firewalld`，记得放行端口：

```bash
sudo firewall-cmd --permanent --add-port=6666/tcp
sudo firewall-cmd --reload
```

## 3. 准备客户端证书和配置

在本地机器上，先拉取服务端公钥证书：

```bash
cd /Users/qiufeihai/workspace/px
VPS_HOST=你的VPS公网IP scripts/fetch-server-cert.sh
```

再生成客户端配置：

```bash
cd /Users/qiufeihai/workspace/px
SERVER_ADDR=你的VPS公网IP:6666 \
SERVER_CERT_PATH=/绝对路径/server-cert.pem \
scripts/create-client-prod-config.sh
```

Windows PowerShell：

```powershell
.\scripts\fetch-server-cert.ps1 -VpsHost 你的VPS公网IP
.\scripts\create-client-prod-config.ps1 -ServerAddr "你的VPS公网IP:6666"
```

关键点：

- `server_addr` 必须写成公网 IP:端口
- `server_cert_path` 推荐写 `config/server-cert.pem`
- `server-key.pem` 只能留在服务端
- 当前客户端按证书文件固定服务端身份，不依赖公网 CA

## 4. 启动 PX 个人代理

正式客户端只有 GUI。

首次使用前，确保当前运行目录下有：

- `config/client.toml`
- `config/server-cert.pem`

然后启动 `PX 个人代理`。

如果你在本地打包 GUI：

```bash
cd /Users/qiufeihai/workspace/px/apps/tauri-ui
npm run tauri build -- --bundles app
```

## 5. 推荐顺序

1. VPS 上部署 `px-server`
2. 本地拉取服务端证书
3. 本地生成客户端配置
4. 启动 `PX 个人代理`
5. 验证链路是否正常

## 额外说明

- 首版只支持 TCP，不支持 UDP
- TUN 通过外部 helper 接到本地 SOCKS5，目前只做全局 TCP
- GUI 只做控制面，不进入代理数据热路径
- 发布与打包见：[client-packaging.md](file:///Users/qiufeihai/workspace/px/docs/client-packaging.md)
- 本地联调见：[quickstart.md](file:///Users/qiufeihai/workspace/px/docs/quickstart.md)
