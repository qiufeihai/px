# TCP-only TUN Helper 设计稿

## 目标

这份文档只回答一件事：

如何把 TUN 路径稳定收口到更短的 `TCP-only helper -> ingress -> runtime`。

约束不变：

- 性能优先，尽量无损接近直连
- 首版只做 TCP，不做 UDP
- GUI 只做控制面，不进入热路径
- 高权限、路由、DNS、TUN 设备仍留在外部 helper 冷路径

## 最终目标形态

后续客户端目标形态是双入口并存：

- 非 TUN 普通代理继续走本地 `SOCKS5 -> runtime`
- TUN 专用链路逐步演进为 `TCP-only helper -> ingress -> runtime`

对应链路：

```text
非 TUN:
应用 / 浏览器
  -> 本地 SOCKS5 listener
  -> socks5.rs
  -> session.rs
  -> upstream.rs
  -> px-server

TUN:
系统 TCP 流量
  -> TUN 设备
  -> px-tun-helper
  -> 本地 ingress listener
  -> ingress.rs
  -> session.rs
  -> upstream.rs
  -> px-server
```

关键原则：

- 不为了优化 TUN，反向让非 TUN 路径多一层 ingress
- `SOCKS5` 继续保留为普通本地代理入口
- `ingress` 只作为 TUN 专用优化入口

## helper 和 runtime 的最终边界

helper 保留：

- TUN 设备读写
- 平台相关能力
- 路由安装/清理
- DNS 切换/恢复
- root 权限相关动作
- 把 TUN 里的 TCP 连接翻译成极薄本地 ingress 会话

runtime 保留：

- 本地入口 listener
- ingress 请求解析
- 上游 TLS 建流
- 协议封装
- 双向转发
- 失败状态映射
- 日志与生命周期管理

一句话：

- helper 只做系统接入
- runtime 只做代理会话

## 为什么不是“所有入口都改 ingress”

如果把非 TUN 的普通本地 SOCKS5 也强行改成：

```text
应用 -> SOCKS5 -> ingress -> runtime
```

那会让非 TUN 路径多出：

- 一次额外本地连接
- 一次额外协议封装
- 一次额外错误映射

这对普通代理是负优化。

因此正确形态必须是：

- 非 TUN 继续 `SOCKS5 -> runtime`
- TUN 单独优化成 `helper -> ingress -> runtime`

## 当前实现

当前仓库已经删除旧的手写 Rust `crates/px-tun-helper` 路线，TUN helper 只保留新的 Go 实现：

```text
helpers/
  px-tun-helper/
    cmd/px-tun-helper/
    internal/config/
    internal/tun/
    internal/netstack/
    internal/ingress/
    internal/bridge/
```

当前实现边界：

- `internal/tun/` 负责 macOS `utun` / Windows `wintun` 设备读写
- `internal/netstack/` 负责 userspace TCP/IP 栈接入
- `internal/ingress/` 复用现有 `px_proto` ingress 协议
- `internal/bridge/` 负责 `TCP conn <-> ingress stream` 双向转发
- GUI 与 `scripts/macos-tun-helper.sh` 继续只做控制面、提权、路由和 DNS 冷路径

当前已验证能力：

- 开发态默认 helper 已切到 Go `px-tun-helper`
- 开发态默认链路为 `px-tun-helper -> ingress -> runtime`
- 真实浏览器流量已跑通 `baidu`、`google`、`github`
- 多次连续运行暂未发现明显稳定性问题
- Windows 默认 helper 名称、Tauri 启动参数和发布链已切向 `px-tun-helper.exe`

当前刻意不做：

- UDP
- IPv6
- 域名保真转发
- fake-ip
- 规则系统
- 为旧手写 helper 保留长期兼容逻辑


## 当前协议与数据流

当前 helper 继续直接复用现有 `px_proto`：

- `ConnectRequest`
- `ConnectResponse`

当前最小数据流如下：

1. 从 `utun` 读取系统 TCP 流量
2. 交给 userspace TCP/IP 栈处理连接
3. 为每条 TCP 连接建立本地 ingress 会话
4. 通过 `internal/bridge/` 做 `TCP conn <-> ingress stream` 双向 copy
5. 由 runtime 继续完成上游建连、协议封装和双向转发

当前阶段默认接受的约束：

- 只支持 IPv4 + TCP
- 当前大多数请求仍按目标 IP 直连
- 目标是缩短 TUN 热路径，不是保留旧手写 TCP 终止细节

## 和现有代码的关系

保留：

- `crates/px-runtime/src/socks5.rs`
- `crates/px-runtime/src/ingress.rs`
- `crates/px-runtime/src/session.rs`
- `crates/px-runtime/src/upstream.rs`

当前不再保留：

- 本地 SOCKS5 bridge 过渡链
- 旧手写 Rust `crates/px-tun-helper`
- 与旧手写 helper 绑定的排障日志和文档说明

## 当前推进顺序

当前推进顺序已经收口为：

1. 保持开发态默认 Go `px-tun-helper`
2. 继续观察冷连接首访时延
3. 补齐 Windows 的 Go helper 覆盖和发布链
4. 评估正式发布默认 helper 的切换时机
5. 继续删除旧手写 helper 和旧 bridge 的冗余残留

## 成功标准

当前阶段成功不看功能多，而看以下几点：

- macOS 下可启动新 helper
- 单个 TCP 连接可穿过 ingress
- 浏览器基础网页可打开
- 停止 TUN 后 DNS/路由能恢复
- 非 TUN SOCKS5 路径完全不受影响

## 一句话总结

`px-tun-helper` 不是新一代通用 `tun2socks`。

它只是一个：

- 只服务 PX
- 只支持 TCP
- 带成熟 TCP/IP 栈
- 直接说 ingress
- 尽量缩短 TUN 热路径

的本地 TUN helper。

## 当前阶段结论

截至当前联调结论，继续在现有 `px-tun-helper` 上手写 TCP 终止细节，已经不是最简单、最快、最稳的路径。

原因不是这条方向完全错误，而是当前复杂度已经越过了“最小可控”边界：

- 外部 helper 这个边界本身是对的，应继续保留
- 复杂的是 helper 内部手写 TCP 终止细节本身
- 现网联调已经证明，这条路线继续靠补兼容性细节收敛，迭代成本高，收益越来越低

一句话：

- `external helper` 形态继续保留
- 当前这套“手写最小 TCP 终止栈”的实现路线应停止继续深挖

## 替代 helper 方向

下一阶段建议保留现有 `helper -> ingress -> runtime` 边界，但替换 helper 内部 TCP 处理核心：

```text
当前保留:
系统 TCP
  -> utun
  -> 成熟 TCP/IP 栈
  -> ingress
  -> runtime
```

关键点：

- 不回退 external helper 形态
- 不把 TUN 协议栈重新塞回 GUI 或 runtime 热路径
- 不改变非 TUN `SOCKS5 -> runtime` 主路径
- 只替换 helper 内部最难收敛的 TCP 终止部分

## 替代 helper 目标边界

替代 helper 仍然只做 TUN 专用能力：

- 打开和管理 `utun`
- 注入/收取 TUN 包
- 调用成熟 TCP/IP 栈处理 TCP 连接
- 把每条已建立 TCP 连接直接桥接到现有 ingress listener
- 继续配合现有路由/DNS/helper 启停逻辑

runtime 继续只做：

- ingress 请求解析
- 上游建连
- 协议封装
- 双向转发
- 生命周期和错误管理

这条边界不变，变化的只是 helper 内部不再自己手写浏览器兼容 TCP 端点。

## 推荐技术路线

优先推荐：

- 新 helper 使用独立实现
- helper 内部使用成熟 TCP/IP 栈
- 继续复用现有 `px_proto` ingress 协议

推荐原则：

- 兼容性优先于“继续保持当前手写骨架”
- 只服务 PX 当前 TUN 路径，不演变成通用平台代理框架
- 只支持 TCP，不顺手扩 UDP/IPv6/规则系统

这意味着新 helper 的价值不是“做更多功能”，而是：

- 用更成熟的 TCP 连接处理替换当前最复杂、最不稳的自研部分

## 新 helper 最小模块

建议替代 helper 的最小模块如下：

```text
cmd/
  px-tun-helper

internal/
  tun/              # utun 设备读写
  netstack/         # 成熟 TCP/IP 栈接入层
  ingress/          # 现有 px ingress client
  bridge/           # TCP conn <-> ingress stream 双向转发
  platform/         # macOS 路由 / DNS / pid / 清理
```

职责收敛：

- `tun/`
  - 只管设备收发
- `netstack/`
  - 只把 TUN 包交给成熟 TCP/IP 栈
  - 只从栈里取出待写回 TUN 的包
- `ingress/`
  - 继续复用 `ConnectRequest / ConnectResponse`
- `bridge/`
  - 对每条 TCP 连接与 ingress stream 做字节流双向 copy
- `platform/`
  - 继续承接当前 macOS helper 冷路径动作

## 迁移步骤

建议按下面顺序推进：

1. 保持当前 Go helper 为开发态默认实现
2. 删除旧手写 helper 的冗余代码、诊断和文档残留
3. 继续优化 `utun -> 成熟 TCP/IP 栈 -> ingress` 冷路径时延
4. 当前阶段只支持：
   - macOS
   - IPv4
   - TCP
5. 保持 runtime 和服务端完全不动
6. 只拿浏览器最关键目标验收：
   - `google`
   - `github`
7. 评估正式发布默认 helper 的切换时机
8. 保持非 TUN 主路径不受影响

## 新阶段成功标准

替代 helper 的最小成功标准不再是“更多诊断”，而是：

- `google` 首页可稳定打开
- `github` 首页可稳定打开
- 多次启停 TUN 后仍稳定
- 停止 TUN 后路由和 DNS 可稳定恢复
- 非 TUN `SOCKS5 -> runtime` 路径完全不受影响

## 迁移期约束

替代 helper 阶段仍必须遵守当前项目约束：

- 不为了 TUN 优化，反向让非 TUN 路径多一层 ingress
- 不把 helper 做成多协议、多控制面的通用平台代理
- 不额外引入与当前范围无关的 UDP/IPv6/规则系统
- 先把浏览器 TCP 场景跑稳，再讨论扩展

## 当前决策

当前文档到此为止，正式记录以下决策：

- 保留 external helper 形态
- 保留 `helper -> ingress -> runtime` 的总体边界
- 停止继续深挖当前手写 TCP 终止实现
- 下一阶段转向“替换 helper 内部 TCP/IP 核心”的更稳路线

## 实施拆分

下一阶段建议直接按下面 3 个阶段推进，每个阶段都只做最小闭环，不预埋大抽象。

### 阶段 1：替代 helper 最小骨架

目标：

- 跑起一个新的外部 helper 进程
- 能打开 macOS `utun`
- 能把 TUN 包交给新的 TCP/IP 栈入口
- 先不要求浏览器可用

建议任务：

1. 新建替代 helper 工程目录和最小入口
2. 接入 macOS `utun` 打开、读写和退出清理
3. 接入新的 TCP/IP 栈初始化
4. 打通 `utun -> TCP/IP 栈` 和 `TCP/IP 栈 -> utun` 的最小包收发
5. 保留最小日志：
   - helper 启动
   - 设备打开成功
   - 栈初始化成功
   - 包收发计数

本阶段验证：

- helper 可启动
- TUN 启停不影响当前 DNS/路由恢复逻辑
- helper 不因普通系统杂包直接退出

### 阶段 2：TCP 连接接 ingress

目标：

- 新 TCP/IP 栈里出现的 TCP 连接，可以直接桥接到现有 ingress
- runtime 和服务端完全不改

建议任务：

1. 为每条新 TCP 连接接入现有 `px_proto` ingress client
2. 继续复用当前 `ConnectRequest / ConnectResponse`
3. 打通：
   - `tcp conn -> ingress stream`
   - `ingress stream -> tcp conn`
4. 先只支持：
   - IPv4
   - TCP
   - 目标 IP 直连
5. 保留最小日志：
   - 新连接建立
   - ingress 建连成功/失败
   - 双向字节数
   - 关闭原因

本阶段验证：

- 单连接 HTTP/HTTPS 可稳定穿过 ingress
- runtime 侧不需要新增针对旧手写 helper 的特殊兼容
- `google/github` 至少出现稳定建立和首包交换

### 阶段 3：接回现有 GUI 和 macOS 启停链

目标：

- 新 helper 取代当前开发态下的手写 `px-tun-helper`
- 对 GUI 来说仍然只是“启动一个 TUN helper”

建议任务：

1. 保持 `scripts/macos-tun-helper.sh` 的主边界不变
2. 让 GUI 开发态可切换到新 helper 二进制
3. 保持：
   - 路由安装/清理
   - DNS helper 启停
   - pid 文件回收
   - 异常退出清理
4. 开发态保持 Go helper 为默认值
5. 正式发布默认值单独评估

本阶段验证：

- GUI 可正常启动/停止 TUN
- 多次启停后 DNS 和路由恢复稳定
- 非 TUN `SOCKS5 -> runtime` 行为完全不变

## 建议仓库改动顺序

为避免一下子动太多，建议按这个顺序落代码：

1. 新增替代 helper 工程和最小 README/设计说明
2. 先打通 `utun <-> 新 TCP/IP 栈`
3. 再打通 `TCP 连接 <-> ingress`
4. 再接回 `macos-tun-helper.sh`
5. 最后再改 Tauri 开发态默认 `helper_path`

## 当前实现落地

当前仓库中 Go `px-tun-helper` 已经落地并接入当前开发态主链路，位置为：

```text
helpers/
  px-tun-helper/
    cmd/px-tun-helper/
    internal/config/
    internal/tun/
    internal/netstack/
    internal/ingress/
    internal/bridge/
```

当前已落内容：

- 新 helper 已独立于 Rust workspace，避免把 Go 构建链混进现有 Rust 热路径
- `internal/tun/` 已接入 `golang.zx2c4.com/wireguard/tun`，用于打开 macOS `utun`
- `internal/netstack/` 已接入 userspace TCP/IP 栈入口，当前只覆盖 IPv4 + TCP
- `internal/ingress/` 与 `internal/bridge/` 已打通 `TCP conn <-> ingress stream` 双向转发
- 当前开发态安装脚本会把 helper 安装到 `apps/tauri-ui/.px-dev-runtime/bin/px-tun-helper`
- 当前开发态 GUI 与 `scripts/macos-tun-helper.sh` 已默认复用该 helper 做联调

当前实现说明：

- 当前默认目标已经从“证明骨架可运行”升级到“保持开发态稳定可用”
- 当前 userspace TCP/IP 栈依赖先使用可直接编译落地的 netstack fork，后续若替换依赖，前提仍是不破坏 `helper -> ingress -> runtime` 边界和非 TUN 主路径

## 当前联调结果

当前替代 helper 已在开发态完成真实浏览器流量联调，结论如下：

- 新的 Go `px-tun-helper` 已可通过现有 GUI + `scripts/macos-tun-helper.sh` 启停链直接运行
- 当前默认形态下，`baidu`、`google`、`github` 已实测可正常访问
- 当前 `helper -> ingress -> runtime` 主链路已验证可持续承载真实 `80/443` 浏览器流量
- 当前自动停掉问题在最近一轮联调中未再复现；helper 未出现新的内部读写错误退出

从 helper 冷路径耗时看：

- 大多数连接的 `accept_to_ingress_ok_ms` 处于几十到几百毫秒
- 仍有少量冷连接会升到约 `1s~3s`
- 少量站点首访仍会感知到偏慢，但当前已经从“功能正确性问题”收敛为“冷启动时延优化问题”

当前阶段判断：

- 不再需要继续大改数据面
- 后续优先做文档收尾和冷路径性能观察
- 若继续优化，应优先盯 `accept_to_ingress_ok_ms` 偏高的个别连接，而不是回退当前 helper 边界

## 当前不做

为了保持范围收敛，替代 helper 第一阶段明确不做：

- UDP
- IPv6
- 域名保真转发
- fake-ip
- 规则系统
- 多平台代理兼容层
- 为旧手写 helper 保留长期双实现兼容逻辑

## 第一批验收站点

建议固定第一批只验下面几类：

- `google`
- `github`
- `1.1.1.1:443`
- 一个当前已经稳定成功的对照 HTTPS 站点

这样可以同时验证：

- 最难目标是否从“不继续发首包”改善为稳定交换
- 新 helper 是否没有把当前已成功链路搞坏
