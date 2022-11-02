package main

import (
	"context"
	"log"

	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/rest"
	gwcl "sigs.k8s.io/gateway-api/pkg/client/clientset/versioned"
)

//go:generate go run github.com/cilium/ebpf/cmd/bpf2go -cc $BPF_CLANG -cflags $BPF_CFLAGS bpf xdp.c -- -I../headers

var (
	objs   *bpfObjects
	router *RoutingData
	cfg    *rest.Config
	gwc    *gwcl.Clientset
	k8s    *kubernetes.Clientset
)

func init() {
	router = NewRouter()

	var err error
	cfg, err = rest.InClusterConfig()
	if err != nil {
		log.Fatalf("could not find kubernetes config: %s", err)
	}

	k8s, err = kubernetes.NewForConfig(cfg)
	if err != nil {
		log.Fatalf("could not create kubernetes client: %s", err)
	}

	gwc, err = gwcl.NewForConfig(cfg)
	if err != nil {
		log.Fatalf("could not create Gateway API kubernetes client: %s", err)
	}
}

func main() {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	if err := startPodController(ctx); err != nil {
		log.Fatalf("ERROR: could not start Pod controller: %s", err)
	}

	if err := startUDPRouteController(ctx); err != nil {
		log.Fatalf("ERROR: could not start UDPRoute controller: %s", err)
	}

	var err error
	objs, err = startXDPLoader(ctx)
	if err != nil {
		log.Fatalf("ERROR: could not load XDP programs: %s", err)
	}

	// FIXME: temporary hack for demo
	sourceADDR, err := hwaddr2bytes("92:d1:b5:2b:dd:50")
	if err != nil {
		log.Fatalf(err.Error())
	}
	destADDR, err := hwaddr2bytes("4e:93:77:36:1a:04")
	if err != nil {
		log.Fatalf(err.Error())
	}
	router.AddInterface(ip2int("10.244.0.8"), BackendInterface{
		InterfaceIndex:   8,
		SrcHardwareAddr:  sourceADDR,
		DestHardwareAddr: destADDR,
	})

	for {
	}
}
