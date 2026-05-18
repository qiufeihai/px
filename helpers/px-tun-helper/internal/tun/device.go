package tun

import (
	"fmt"

	wgtun "golang.zx2c4.com/wireguard/tun"
)

type Device struct {
	dev      wgtun.Device
	name     string
	mtu      int
	readBuf  []byte
	writeBuf []byte
}

func Open(name string, mtu int) (*Device, error) {
	dev, err := wgtun.CreateTUN(name, mtu)
	if err != nil {
		return nil, fmt.Errorf("create tun: %w", err)
	}

	actualName, err := dev.Name()
	if err != nil {
		_ = dev.Close()
		return nil, fmt.Errorf("read tun name: %w", err)
	}

	actualMTU, err := dev.MTU()
	if err != nil {
		actualMTU = mtu
	}

	return &Device{
		dev:  dev,
		name: actualName,
		mtu:  actualMTU,
	}, nil
}

func (d *Device) Name() string {
	return d.name
}

func (d *Device) MTU() int {
	return d.mtu
}

func (d *Device) ReadPacket(buf []byte) (int, error) {
	raw := d.ensureReadBuf(len(buf))
	bufs := [][]byte{raw}
	var sizes [1]int
	_, err := d.dev.Read(bufs, sizes[:], platformPacketOffset)
	if err != nil {
		return 0, err
	}
	n := sizes[0]
	copy(buf, raw[platformPacketOffset:platformPacketOffset+n])
	return n, nil
}

func (d *Device) WritePacket(pkt []byte) error {
	if len(pkt) == 0 {
		return nil
	}
	raw := d.ensureWriteBuf(len(pkt))
	copy(raw[platformPacketOffset:], pkt)
	_, err := d.dev.Write([][]byte{raw}, platformPacketOffset)
	return err
}

func (d *Device) Close() error {
	return d.dev.Close()
}

func (d *Device) ensureReadBuf(payloadLen int) []byte {
	totalLen := payloadLen + platformPacketOffset
	if cap(d.readBuf) < totalLen {
		d.readBuf = make([]byte, totalLen)
	} else {
		d.readBuf = d.readBuf[:totalLen]
	}
	return d.readBuf
}

func (d *Device) ensureWriteBuf(payloadLen int) []byte {
	totalLen := payloadLen + platformPacketOffset
	if cap(d.writeBuf) < totalLen {
		d.writeBuf = make([]byte, totalLen)
	} else {
		d.writeBuf = d.writeBuf[:totalLen]
	}
	return d.writeBuf
}
