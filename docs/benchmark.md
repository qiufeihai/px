# PX 基准测试说明

## 目的

- 定量比较直连与当前代理实现的连接建立时延差异。
- 作为后续是否需要更复杂优化的依据。

## 当前工具

直接运行：

```bash
cargo run -p px-bench -- --socks 127.0.0.1:7777 --target example.com:80 --iterations 10
```

也可以使用包装脚本：

```bash
scripts/run-bench.sh
```

默认值：

- `SOCKS_ADDR=127.0.0.1:7777`
- `TARGET=example.com:80`
- `ITERATIONS=20`

常用示例：

```bash
TARGET=1.1.1.1:80 ITERATIONS=50 scripts/run-bench.sh
```

```bash
SOCKS_ADDR=127.0.0.1:7777 TARGET=example.com:443 ITERATIONS=30 scripts/run-bench.sh
```

浏览器场景可用多域名脚本：

```bash
scripts/run-browser-bench.sh
```

默认会轮询这些目标：

- `cloudflare.com:443`
- `github.com:443`
- `www.apple.com:443`
- `www.microsoft.com:443`
- `www.wikipedia.org:443`

也可以快速覆盖目标列表：

```bash
BROWSER_TARGETS="github.com:443 www.apple.com:443" ITERATIONS=20 scripts/run-browser-bench.sh
```

如果要做“域名 vs IP”对照，也可以额外给一组手工 IP：

```bash
BROWSER_TARGETS="github.com:443 www.apple.com:443" \
BROWSER_IP_TARGETS="140.82.114.4:443 17.253.144.10:443" \
ITERATIONS=20 \
scripts/run-browser-bench.sh
```

这里的 `BROWSER_IP_TARGETS` 需要你自己提供对应站点当前可用的 IP，用于粗看 DNS 解析是否可能是主要波动来源。

默认会连续执行多轮：

- `ROUNDS=3`

例如：

```bash
ROUNDS=5 ITERATIONS=20 scripts/run-browser-bench.sh
```

输出：

- `direct_avg_ms`：目标地址直连平均建连时延
- `direct_p95_ms` / `direct_p99_ms`：目标地址直连建连尾延迟
- `socks_avg_ms`：通过本地 SOCKS5 代理后的平均建连时延
- `socks_p95_ms` / `socks_p99_ms`：通过本地 SOCKS5 代理后的建连尾延迟
- `delta_ms`：二者差值

建议先看：

- `delta_ms`：平均额外开销是否明显
- `socks_p95_ms` / `socks_p99_ms`：是否存在尾延迟抖动
- 同一目标多跑几轮，避免一次性结果误判

浏览器多域名脚本还会额外输出：

- `success` / `fail`：成功与失败目标数量
- `rounds`：实际汇总的轮数
- `*_mean_ms`：跨多个目标的平均结果
- `failed_targets`：失败目标列表，方便排除临时网络问题
- `per_target_summary`：按目标汇总的均值，方便识别某个站点是否持续抖动
- `domain_summary`：仅域名目标的汇总
- `ip_summary`：仅 IP 对照目标的汇总

判断是否值得做轻量 DNS 降抖时，优先看：

- 同一轮里 `domain_summary` 是否持续明显慢于 `ip_summary`
- 同类目标下，域名组的 `socks_p95_ms` / `socks_p99_ms` 是否持续更差
- 多跑几轮后差异是否仍然稳定，而不是单轮偶发波动

## 推荐后续补充

- 长连接吞吐对比
- 并发短连接压测
- 与 `4px` 的同机对照
- Rocky9 服务端 CPU 占用与尾延迟采样
