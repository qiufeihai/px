package ingress

import (
	"context"
	"encoding/binary"
	"fmt"
	"io"
	"net"
	"net/netip"
	"time"
)

var magic = [4]byte{'P', 'X', 'T', '1'}

type Connector struct {
	ingressAddr string
	timeout     time.Duration
}

type ConnectMetrics struct {
	DialElapsed    time.Duration
	RequestElapsed time.Duration
	TotalElapsed   time.Duration
}

func NewConnector(ingressAddr string, timeout time.Duration) *Connector {
	return &Connector{
		ingressAddr: ingressAddr,
		timeout:     timeout,
	}
}

func (c *Connector) ConnectTarget(ctx context.Context, target netip.AddrPort) (net.Conn, ConnectMetrics, error) {
	startedAt := time.Now()
	dialer := &net.Dialer{Timeout: c.timeout}
	conn, err := dialer.DialContext(ctx, "tcp", c.ingressAddr)
	if err != nil {
		return nil, ConnectMetrics{}, fmt.Errorf("connect ingress %s: %w", c.ingressAddr, err)
	}
	dialElapsed := time.Since(startedAt)

	if deadline, ok := ctx.Deadline(); ok {
		_ = conn.SetDeadline(deadline)
	} else if c.timeout > 0 {
		_ = conn.SetDeadline(time.Now().Add(c.timeout))
	}

	requestStartedAt := time.Now()
	if err := writeConnectRequest(conn, target); err != nil {
		_ = conn.Close()
		return nil, ConnectMetrics{}, err
	}

	if err := readConnectResponse(conn, target); err != nil {
		_ = conn.Close()
		return nil, ConnectMetrics{}, err
	}

	_ = conn.SetDeadline(time.Time{})
	return conn, ConnectMetrics{
		DialElapsed:    dialElapsed,
		RequestElapsed: time.Since(requestStartedAt),
		TotalElapsed:   time.Since(startedAt),
	}, nil
}

func writeConnectRequest(conn net.Conn, target netip.AddrPort) error {
	addr := target.Addr()
	if !addr.Is4() {
		return fmt.Errorf("unsupported target address: %s", target.String())
	}

	buf := make([]byte, 12)
	copy(buf[:4], magic[:])
	buf[4] = 1
	buf[5] = 1
	buf[6] = 1
	buf[7] = 0
	binary.BigEndian.PutUint16(buf[8:10], target.Port())
	binary.BigEndian.PutUint16(buf[10:12], 0)

	if err := writeAll(conn, buf); err != nil {
		return fmt.Errorf("write ingress connect request for %s: %w", target, err)
	}

	ip4 := addr.As4()
	if err := writeAll(conn, ip4[:]); err != nil {
		return fmt.Errorf("write ingress target ip for %s: %w", target, err)
	}
	return nil
}

func readConnectResponse(conn net.Conn, target netip.AddrPort) error {
	var resp [4]byte
	if _, err := io.ReadFull(conn, resp[:]); err != nil {
		return fmt.Errorf("read ingress connect response for %s: %w", target, err)
	}
	if resp[0] != 0 {
		return fmt.Errorf("ingress refused %s with status=%d reason=%d", target, resp[0], binary.BigEndian.Uint16(resp[2:4]))
	}
	return nil
}

func writeAll(conn net.Conn, payload []byte) error {
	for len(payload) > 0 {
		n, err := conn.Write(payload)
		if err != nil {
			return err
		}
		payload = payload[n:]
	}
	return nil
}
