# PX 基准测试说明

## 目的

- 定量比较直连与当前代理实现的连接建立时延差异。
- 作为后续是否需要更复杂优化的依据。

## 当前工具

运行：

```bash
cargo run -p px-bench -- --socks 127.0.0.1:1080 --target example.com:80 --iterations 10
```

输出：

- `direct_avg_ms`：目标地址直连平均建连时延
- `socks_avg_ms`：通过本地 SOCKS5 代理后的平均建连时延
- `delta_ms`：二者差值

## 推荐后续补充

- 长连接吞吐对比
- 并发短连接压测
- 与 `4px` 的同机对照
- Rocky9 服务端 CPU 占用与尾延迟采样
