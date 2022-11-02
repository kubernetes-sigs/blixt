package main

import "sync"

type BackendInterface struct {
	InterfaceIndex   uint16
	SrcHardwareAddr  [6]uint8
	DestHardwareAddr [6]uint8
}

type RoutingData struct {
	lock    sync.RWMutex
	hwaddrs map[uint32]BackendInterface
}

func NewRouter() *RoutingData {
	return &RoutingData{
		lock:    sync.RWMutex{},
		hwaddrs: make(map[uint32]BackendInterface),
	}
}

func (b *RoutingData) AddInterface(ip uint32, iface BackendInterface) {
	b.lock.Lock()
	defer b.lock.Unlock()
	b.hwaddrs[ip] = iface
}

func (b *RoutingData) GetInterface(ip uint32) (BackendInterface, bool) {
	b.lock.RLock()
	defer b.lock.RUnlock()
	hwaddr, ok := b.hwaddrs[ip]
	return hwaddr, ok
}

func (b *RoutingData) DeleteInterface(ip uint32) {
	b.lock.Lock()
	defer b.lock.Unlock()
	delete(b.hwaddrs, ip)
}
