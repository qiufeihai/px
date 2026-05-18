package netstack

import (
	"context"
	"fmt"
	"net"
	"net/netip"

	"github.com/sagernet/gvisor/pkg/buffer"
	"github.com/sagernet/gvisor/pkg/tcpip"
	"github.com/sagernet/gvisor/pkg/tcpip/adapters/gonet"
	"github.com/sagernet/gvisor/pkg/tcpip/header"
	"github.com/sagernet/gvisor/pkg/tcpip/link/channel"
	"github.com/sagernet/gvisor/pkg/tcpip/network/ipv4"
	"github.com/sagernet/gvisor/pkg/tcpip/stack"
	"github.com/sagernet/gvisor/pkg/tcpip/transport/tcp"
	"github.com/sagernet/gvisor/pkg/waiter"
)

const (
	defaultOutboundQueue = 1024
	defaultNICID         = tcpip.NICID(1)
)

type Engine struct {
	stack    *stack.Stack
	endpoint *channel.Endpoint
	nicID    tcpip.NICID
}

func New(mtu int, addr netip.Addr) (*Engine, error) {
	s := stack.New(stack.Options{
		NetworkProtocols:   []stack.NetworkProtocolFactory{ipv4.NewProtocol},
		TransportProtocols: []stack.TransportProtocolFactory{tcp.NewProtocol},
	})

	ep := channel.New(defaultOutboundQueue, uint32(mtu), "")
	if err := s.CreateNIC(defaultNICID, ep); err != nil {
		return nil, fmt.Errorf("create nic: %s", err)
	}
	if err := s.SetPromiscuousMode(defaultNICID, true); err != nil {
		return nil, fmt.Errorf("enable promiscuous mode: %s", err)
	}
	if err := s.SetSpoofing(defaultNICID, true); err != nil {
		return nil, fmt.Errorf("enable spoofing: %s", err)
	}

	if err := s.AddProtocolAddress(defaultNICID, tcpip.ProtocolAddress{
		Protocol: ipv4.ProtocolNumber,
		AddressWithPrefix: tcpip.AddressWithPrefix{
			Address:   tcpip.AddrFrom4(addr.As4()),
			PrefixLen: 32,
		},
	}, stack.AddressProperties{}); err != nil {
		return nil, fmt.Errorf("add protocol address: %s", err)
	}

	s.SetRouteTable([]tcpip.Route{
		{
			Destination: header.IPv4EmptySubnet,
			NIC:         defaultNICID,
		},
	})

	return &Engine{
		stack:    s,
		endpoint: ep,
		nicID:    defaultNICID,
	}, nil
}

func (e *Engine) AttachTCPForwarder(handler func(net.Conn, netip.AddrPort, netip.AddrPort)) {
	forwarder := tcp.NewForwarder(e.stack, 0, 1024, func(req *tcp.ForwarderRequest) {
		id := req.ID()

		target, err := addrPortFromTCPIP(id.LocalAddress, id.LocalPort)
		if err != nil {
			req.Complete(true)
			return
		}
		source, err := addrPortFromTCPIP(id.RemoteAddress, id.RemotePort)
		if err != nil {
			req.Complete(true)
			return
		}

		var wq waiter.Queue
		ep, tcpErr := req.CreateEndpoint(&wq)
		if tcpErr != nil {
			req.Complete(true)
			return
		}
		req.Complete(false)
		handler(gonet.NewTCPConn(&wq, ep), target, source)
	})

	e.stack.SetTransportProtocolHandler(tcp.ProtocolNumber, forwarder.HandlePacket)
}

func addrPortFromTCPIP(addr tcpip.Address, port uint16) (netip.AddrPort, error) {
	addrSlice := addr.AsSlice()
	ip, ok := netip.AddrFromSlice(addrSlice)
	if !ok || !ip.Is4() {
		return netip.AddrPort{}, fmt.Errorf("unsupported tcpip address")
	}
	return netip.AddrPortFrom(ip, port), nil
}

func (e *Engine) InjectInbound(pkt []byte) error {
	if len(pkt) == 0 {
		return nil
	}
	if header.IPVersion(pkt) != header.IPv4Version {
		return fmt.Errorf("unsupported ip version: %d", header.IPVersion(pkt))
	}

	packet := stack.NewPacketBuffer(stack.PacketBufferOptions{
		Payload: buffer.MakeWithData(append([]byte(nil), pkt...)),
	})
	e.endpoint.InjectInbound(ipv4.ProtocolNumber, packet)
	return nil
}

func (e *Engine) ReadOutbound(ctx context.Context) ([]byte, error) {
	packet := e.endpoint.ReadContext(ctx)
	if packet == nil {
		return nil, context.Canceled
	}
	defer packet.DecRef()

	buf := packet.ToBuffer()
	return buf.Flatten(), nil
}
