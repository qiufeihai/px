package config

import (
	"flag"
	"fmt"
	"net/netip"
	"time"
)

type Config struct {
	Device         string
	MTU            int
	TunIPv4        netip.Addr
	PrimaryInterface string
	RealDevice     bool
	IngressAddr    string
	ConnectTimeout time.Duration
	LogLevel       string
}

func Parse() (Config, error) {
	var (
		device             = flag.String("device", "", "tun device name, for example utun233 or wintun")
		mtu                = flag.Int("mtu", 1500, "tun mtu")
		tunIPv4            = flag.String("tun-ipv4", "", "local tun ipv4 address")
		primaryInterface   = flag.String("primary-interface", "", "primary interface name, accepted for launcher compatibility")
		realDevice         = flag.Bool("real-device", false, "accepted for launcher compatibility")
		ingressAddr        = flag.String("ingress", "127.0.0.1:7778", "local ingress listener address")
		connectTimeoutMs   = flag.Int("connect-timeout-ms", 5000, "ingress connect timeout in milliseconds")
		logLevel           = flag.String("log-level", "", "log level")
		legacyLogLevel     = flag.String("loglevel", "", "legacy log level alias")
	)

	flag.Parse()

	if *device == "" {
		return Config{}, fmt.Errorf("missing --device")
	}
	if *mtu <= 0 {
		return Config{}, fmt.Errorf("invalid --mtu: %d", *mtu)
	}
	if *tunIPv4 == "" {
		return Config{}, fmt.Errorf("missing --tun-ipv4")
	}

	addr, err := netip.ParseAddr(*tunIPv4)
	if err != nil {
		return Config{}, fmt.Errorf("parse --tun-ipv4: %w", err)
	}
	if !addr.Is4() {
		return Config{}, fmt.Errorf("--tun-ipv4 must be an ipv4 address")
	}
	if *ingressAddr == "" {
		return Config{}, fmt.Errorf("missing --ingress")
	}
	if *connectTimeoutMs <= 0 {
		return Config{}, fmt.Errorf("invalid --connect-timeout-ms: %d", *connectTimeoutMs)
	}

	resolvedLogLevel := "info"
	switch {
	case *logLevel != "":
		resolvedLogLevel = *logLevel
	case *legacyLogLevel != "":
		resolvedLogLevel = *legacyLogLevel
	}

	return Config{
		Device:           *device,
		MTU:              *mtu,
		TunIPv4:          addr,
		PrimaryInterface: *primaryInterface,
		RealDevice:       *realDevice,
		IngressAddr:      *ingressAddr,
		ConnectTimeout: time.Duration(*connectTimeoutMs) * time.Millisecond,
		LogLevel:         resolvedLogLevel,
	}, nil
}
