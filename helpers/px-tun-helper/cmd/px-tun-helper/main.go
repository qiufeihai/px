package main

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/netip"
	"os"
	"os/signal"
	"sync"
	"sync/atomic"
	"syscall"
	"time"

	"px/helpers/px-tun-helper/internal/bridge"
	"px/helpers/px-tun-helper/internal/config"
	"px/helpers/px-tun-helper/internal/ingress"
	"px/helpers/px-tun-helper/internal/netstack"
	"px/helpers/px-tun-helper/internal/tun"
)

func main() {
	cfg, err := config.Parse()
	if err != nil {
		slog.Error("invalid config", "err", err)
		os.Exit(2)
	}

	logger := newLogger(cfg.LogLevel)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	signalCh := make(chan os.Signal, 1)
	signal.Notify(signalCh, os.Interrupt, syscall.SIGTERM)
	defer signal.Stop(signalCh)

	var stopOnce sync.Once
	var stopReason atomic.Value
	stopReason.Store("unknown")

	stop := func(reason string) {
		stopOnce.Do(func() {
			stopReason.Store(reason)
			cancel()
		})
	}

	go func() {
		select {
		case sig := <-signalCh:
			logger.Info("received stop signal", "signal", sig.String())
			stop("signal:" + sig.String())
		case <-ctx.Done():
		}
	}()

	device, err := tun.Open(cfg.Device, cfg.MTU)
	if err != nil {
		logger.Error("open tun failed", "device", cfg.Device, "err", err)
		os.Exit(1)
	}
	defer device.Close()

	engine, err := netstack.New(device.MTU(), cfg.TunIPv4)
	if err != nil {
		logger.Error("init netstack failed", "err", err)
		os.Exit(1)
	}
	connector := ingress.NewConnector(cfg.IngressAddr, cfg.ConnectTimeout)
	engine.AttachTCPForwarder(func(clientConn net.Conn, target netip.AddrPort, source netip.AddrPort) {
		acceptedAt := time.Now()
		logger.Debug("tcp flow accepted", "target", target.String(), "source", source.String())

		ingressConn, metrics, err := connector.ConnectTarget(ctx, target)
		if err != nil {
			logger.Error("connect ingress target failed", "target", target.String(), "source", source.String(), "err", err)
			_ = clientConn.Close()
			return
		}

		ingressReadyAt := time.Now()
		logger.Debug("ingress target connected",
			"target", target.String(),
			"source", source.String(),
			"accept_to_ingress_ok_ms", ingressReadyAt.Sub(acceptedAt).Milliseconds(),
			"ingress_dial_ms", metrics.DialElapsed.Milliseconds(),
			"ingress_request_ms", metrics.RequestElapsed.Milliseconds(),
			"ingress_total_ms", metrics.TotalElapsed.Milliseconds(),
		)

		go bridge.BridgeTCP(ctx, logger, target, clientConn, ingressConn, bridge.FlowTiming{
			AcceptedAt:     acceptedAt,
			IngressReadyAt: ingressReadyAt,
		})
	})

	logger.Info("helper started",
		"device", device.Name(),
		"mtu", device.MTU(),
		"tun_ipv4", cfg.TunIPv4.String(),
		"ingress", cfg.IngressAddr,
	)

	var inboundPackets atomic.Uint64
	var outboundPackets atomic.Uint64

	go func() {
		buf := make([]byte, device.MTU()+256)
		for {
			n, err := device.ReadPacket(buf)
			if err != nil {
				if ctx.Err() != nil {
					return
				}
				logger.Error("tun read failed", "err", err)
				stop("tun read failed")
				return
			}

			if n == 0 {
				continue
			}

			if err := engine.InjectInbound(buf[:n]); err != nil {
				logger.Debug("drop inbound packet", "err", err, "bytes", n)
				continue
			}

			total := inboundPackets.Add(1)
			if total == 1 || total%256 == 0 {
				logger.Info("inbound packets observed", "packets", total)
			}
		}
	}()

	go func() {
		for {
			pkt, err := engine.ReadOutbound(ctx)
			if err != nil {
				if errors.Is(err, context.Canceled) || ctx.Err() != nil {
					return
				}
				logger.Error("netstack outbound read failed", "err", err)
				stop("netstack outbound read failed")
				return
			}

			if len(pkt) == 0 {
				continue
			}

			if err := device.WritePacket(pkt); err != nil {
				if ctx.Err() != nil {
					return
				}
				logger.Error("tun write failed", "err", err, "bytes", len(pkt))
				stop("tun write failed")
				return
			}

			total := outboundPackets.Add(1)
			if total == 1 || total%256 == 0 {
				logger.Info("outbound packets emitted", "packets", total)
			}
		}
	}()

	<-ctx.Done()
	logger.Info("helper stopping",
		"reason", stopReason.Load(),
		"inbound_packets", inboundPackets.Load(),
		"outbound_packets", outboundPackets.Load(),
	)
}

func newLogger(level string) *slog.Logger {
	var slogLevel slog.Level
	switch level {
	case "debug":
		slogLevel = slog.LevelDebug
	case "warn":
		slogLevel = slog.LevelWarn
	case "error":
		slogLevel = slog.LevelError
	default:
		slogLevel = slog.LevelInfo
	}

	return slog.New(slog.NewTextHandler(os.Stdout, &slog.HandlerOptions{
		Level: slogLevel,
	}))
}
