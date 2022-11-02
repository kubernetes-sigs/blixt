package main

import (
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"net"
	"strings"

	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
)

func ip2int(ip string) uint32 {
	ipaddr := net.ParseIP(ip)
	return binary.LittleEndian.Uint32(ipaddr.To4())
}

func hwaddr2bytes(hwaddr string) ([6]byte, error) {
	var hwaddrBytes [6]byte

	parts := strings.Split(hwaddr, ":")
	if len(parts) != 6 {
		return hwaddrBytes, fmt.Errorf("invalid hardware address: %s", hwaddr)
	}

	for i, hexPart := range parts {
		bs, err := hex.DecodeString(hexPart)
		if err != nil {
			return hwaddrBytes, err
		}
		if len(bs) != 1 {
			return hwaddrBytes, fmt.Errorf("invalid hardware address part")
		}
		hwaddrBytes[i] = bs[0]
	}

	return hwaddrBytes, nil
}

func nsn(udproute *gatewayv1alpha2.UDPRoute) string {
	return udproute.Namespace + "/" + udproute.Name
}
