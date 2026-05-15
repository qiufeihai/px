# PX 私有协议说明

## 设计目标

- 仅服务个人场景。
- 首版只做 TCP，不做 UDP。
- 不做多路复用，避免共享 TCP 链路带来的跨流抖动。
- TLS 之上只传一个极简首包，之后字节流透明转发。

## 外层传输

- 传输层：`TCP + TLS 1.3`
- 服务端证书：自签证书
- 客户端信任方式：固定服务端证书文件
- 当前默认要求：客户端会把 `server_cert_path` 指向的证书作为服务端叶子证书精确固定；服务端若换证书，客户端必须同步更新该文件
- 配套生成脚本：开发联调用 `scripts/generate-dev-cert.sh`，VPS 用 `deploy/generate-vps-cert.sh`

## 首包格式

```text
magic[4]   = "PXT1"
version    = u8
cmd        = u8
addr_type  = u8
reserved   = u8
port       = u16
addr_len   = u16
addr_bytes = [u8]
```

- `cmd = 1` 表示 `CONNECT`
- `addr_type = 1` 表示 IPv4
- `addr_type = 2` 表示 IPv6
- `addr_type = 3` 表示域名

## 服务端回包

```text
status   = u8
reserved = u8
reason   = u16
```

- `0 = ok`
- `1 = bad_request`
- `2 = tls_auth_failed`
- `3 = target_connect_failed`
- `4 = timeout`
- `5 = internal_error`
