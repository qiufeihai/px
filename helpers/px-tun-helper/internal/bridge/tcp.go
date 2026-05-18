package bridge

import (
	"context"
	"log/slog"
	"net"
	"net/netip"
	"time"
)

type FlowTiming struct {
	AcceptedAt     time.Time
	IngressReadyAt time.Time
}

func BridgeTCP(
	ctx context.Context,
	logger *slog.Logger,
	target netip.AddrPort,
	client net.Conn,
	ingress net.Conn,
	timing FlowTiming,
) {
	defer client.Close()
	defer ingress.Close()

	done := make(chan struct{})
	go func() {
		select {
		case <-ctx.Done():
			_ = client.Close()
			_ = ingress.Close()
		case <-done:
		}
	}()
	defer close(done)

	results := make(chan result, 2)
	go copyOne(results, "client_to_ingress", ingress, client)
	go copyOne(results, "ingress_to_client", client, ingress)

	first := <-results
	second := <-results

	logger.Debug("tcp bridge finished",
		"target", target.String(),
		"accept_to_ingress_ok_ms", elapsedMillis(timing.AcceptedAt, timing.IngressReadyAt),
		"ingress_ok_to_first_client_byte_ms", elapsedMillis(timing.IngressReadyAt, firstByteForDirection(first, second, "client_to_ingress")),
		"ingress_ok_to_first_upstream_byte_ms", elapsedMillis(timing.IngressReadyAt, firstByteForDirection(first, second, "ingress_to_client")),
		"accept_to_first_upstream_byte_ms", elapsedMillis(timing.AcceptedAt, firstByteForDirection(first, second, "ingress_to_client")),
		"ingress_ok_to_finish_ms", elapsedMillis(timing.IngressReadyAt, time.Now()),
		"client_to_ingress_bytes", bytesForDirection(first, second, "client_to_ingress"),
		"ingress_to_client_bytes", bytesForDirection(first, second, "ingress_to_client"),
		"first_done", first.direction,
		"first_err", printableError(first.err),
		"second_err", printableError(second.err),
	)
}

func copyOne(results chan<- result, direction string, dst net.Conn, src net.Conn) {
	buf := make([]byte, 32*1024)
	var total int64
	var firstByteAt time.Time

	for {
		n, readErr := src.Read(buf)
		if n > 0 {
			if firstByteAt.IsZero() {
				firstByteAt = time.Now()
			}
			written := 0
			for written < n {
				m, writeErr := dst.Write(buf[written:n])
				total += int64(m)
				if writeErr != nil {
					results <- result{
						direction:   direction,
						bytes:       total,
						err:         writeErr,
						firstByteAt: firstByteAt,
					}
					return
				}
				written += m
			}
		}

		if readErr != nil {
			results <- result{
				direction:   direction,
				bytes:       total,
				err:         readErr,
				firstByteAt: firstByteAt,
			}
			return
		}
	}
}

func firstByteForDirection(a result, b result, direction string) time.Time {
	if a.direction == direction {
		return a.firstByteAt
	}
	return b.firstByteAt
}

func elapsedMillis(start time.Time, end time.Time) int64 {
	if start.IsZero() || end.IsZero() {
		return -1
	}
	return end.Sub(start).Milliseconds()
}

func printableError(err error) string {
	if err == nil {
		return ""
	}
	if ne, ok := err.(net.Error); ok && ne.Timeout() {
		return err.Error()
	}
	if err.Error() == "EOF" {
		return ""
	}
	return err.Error()
}

type result struct {
	direction   string
	bytes       int64
	err         error
	firstByteAt time.Time
}

func bytesForDirection(a result, b result, direction string) int64 {
	if a.direction == direction {
		return a.bytes
	}
	return b.bytes
}
