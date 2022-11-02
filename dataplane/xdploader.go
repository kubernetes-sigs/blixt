package main

import (
	"context"
	"fmt"
	"log"
	"net"
	"os"

	"github.com/cilium/ebpf/link"
)

func startXDPLoader(ctx context.Context) (*bpfObjects, error) {
	if len(os.Args) < 2 {
		return nil, fmt.Errorf("Please specify a network interface")
	}

	ifaceName := os.Args[1]
	iface, err := net.InterfaceByName(ifaceName)
	if err != nil {
		return nil, fmt.Errorf("lookup network iface %q: %s", ifaceName, err)
	}

	objs := bpfObjects{}
	if err := loadBpfObjects(&objs, nil); err != nil {
		return nil, fmt.Errorf("loading objects: %s", err)
	}

	l, err := link.AttachXDP(link.XDPOptions{
		Program:   objs.XdpProgFunc,
		Interface: iface.Index,
	})
	if err != nil {
		objs.Close()
		return &objs, fmt.Errorf("could not attach XDP program: %s", err)
	}

	go func() {
		defer objs.Close()
		defer l.Close()

		log.Printf("Attached XDP program to iface %q (index %d)", iface.Name, iface.Index)

		<-ctx.Done()
	}()

	return &objs, nil
}
