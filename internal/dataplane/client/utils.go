package client

import (
	context "context"
	"encoding/binary"
	"fmt"
	"net"

	corev1 "k8s.io/api/core/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

// CompileUDPRouteToDataPlaneBackend takes a UDPRoute and the Gateway it is
// attached to and produces Backend Targets for the DataPlane to configure.
func CompileUDPRouteToDataPlaneBackend(ctx context.Context, c client.Client, udproute *gatewayv1alpha2.UDPRoute, gateway *gatewayv1beta1.Gateway) (*Targets, error) {
	// TODO: add support for multiple rules https://github.com/Kong/blixt/issues/10
	if len(udproute.Spec.Rules) != 1 {
		return nil, fmt.Errorf("currently can only support 1 UDPRoute rule, received %d", len(udproute.Spec.Rules))
	}
	rule := udproute.Spec.Rules[0]

	// TODO: add support for multiple rules https://github.com/Kong/blixt/issues/10
	if len(rule.BackendRefs) != 1 {
		return nil, fmt.Errorf("expect 1 backendRef received %d", len(rule.BackendRefs))
	}
	backendRef := rule.BackendRefs[0]

	gatewayIP, err := getGatewayIP(gateway)
	if gatewayIP == nil {
		return nil, err
	}

	gatewayPort, err := getGatewayPort(gateway, udproute.Spec.ParentRefs)
	if err != nil {
		return nil, err
	}

	// TODO only using one endpoint for now until https://github.com/Kong/blixt/issues/10
	var target *Target
	if udproute.DeletionTimestamp == nil {
		endpoints, err := endpointsFromBackendRef(ctx, c, udproute.Namespace, backendRef)
		if err != nil {
			return nil, err
		}

		for _, subset := range endpoints.Subsets {
			if len(subset.Addresses) < 1 {
				return nil, fmt.Errorf("addresses not ready for endpoints")
			}
			if len(subset.Ports) < 1 {
				return nil, fmt.Errorf("ports not ready for endpoints")
			}

			if subset.Addresses[0].IP == "" {
				return nil, fmt.Errorf("empty IP for endpoint subset")
			}

			ip := net.ParseIP(subset.Addresses[0].IP)

			podip := binary.BigEndian.Uint32(ip.To4())

			target = &Target{
				Daddr: podip,
				Dport: uint32(subset.Ports[0].Port),
			}
		}
		if target == nil {
			return nil, fmt.Errorf("endpoints not ready")
		}
	}

	ipint := binary.BigEndian.Uint32(gatewayIP.To4())

	targets := &Targets{
		Vip: &Vip{
			Ip:   ipint,
			Port: gatewayPort,
		},
		Target: target,
	}

	return targets, nil
}

// CompileTCPRouteToDataPlaneBackend takes a TCPRoute and the Gateway it is
// attached to and produces Backend Targets for the DataPlane to configure.
func CompileTCPRouteToDataPlaneBackend(ctx context.Context, c client.Client, tcproute *gatewayv1alpha2.TCPRoute, gateway *gatewayv1beta1.Gateway) (*Targets, error) {
	// TODO: add support for multiple rules https://github.com/Kong/blixt/issues/10
	if len(tcproute.Spec.Rules) != 1 {
		return nil, fmt.Errorf("currently can only support 1 TCPRoute rule, received %d", len(tcproute.Spec.Rules))
	}
	rule := tcproute.Spec.Rules[0]

	// TODO: add support for multiple rules https://github.com/Kong/blixt/issues/10
	if len(rule.BackendRefs) != 1 {
		return nil, fmt.Errorf("expect 1 backendRef received %d", len(rule.BackendRefs))
	}
	backendRef := rule.BackendRefs[0]

	gatewayIP, err := getGatewayIP(gateway)
	if gatewayIP == nil {
		return nil, err
	}

	gatewayPort, err := getGatewayPort(gateway, tcproute.Spec.ParentRefs)
	if err != nil {
		return nil, err
	}

	// TODO only using one endpoint for now until https://github.com/Kong/blixt/issues/10
	var target *Target
	if tcproute.DeletionTimestamp == nil {
		endpoints, err := endpointsFromBackendRef(ctx, c, tcproute.Namespace, backendRef)
		if err != nil {
			return nil, err
		}

		for _, subset := range endpoints.Subsets {
			if len(subset.Addresses) < 1 {
				return nil, fmt.Errorf("addresses not ready for endpoints")
			}
			if len(subset.Ports) < 1 {
				return nil, fmt.Errorf("ports not ready for endpoints")
			}

			if subset.Addresses[0].IP == "" {
				return nil, fmt.Errorf("empty IP for endpoint subset")
			}

			ip := net.ParseIP(subset.Addresses[0].IP)

			podip := binary.BigEndian.Uint32(ip.To4())

			target = &Target{
				Daddr: podip,
				Dport: uint32(subset.Ports[0].Port),
			}
		}
		if target == nil {
			return nil, fmt.Errorf("endpoints not ready")
		}
	}

	ipint := binary.BigEndian.Uint32(gatewayIP.To4())

	targets := &Targets{
		Vip: &Vip{
			Ip:   ipint,
			Port: gatewayPort,
		},
		Target: target,
	}

	return targets, nil
}

func endpointsFromBackendRef(ctx context.Context, c client.Client, namespace string, backendRef gatewayv1alpha2.BackendRef) (*corev1.Endpoints, error) {
	if backendRef.Namespace != nil {
		namespace = string(*backendRef.Namespace)
	}

	endpoints := new(corev1.Endpoints)
	if err := c.Get(ctx, client.ObjectKey{
		Namespace: namespace,
		Name:      string(backendRef.Name),
	}, endpoints); err != nil {
		return nil, err
	}

	return endpoints, nil
}

func getGatewayIP(gw *gatewayv1beta1.Gateway) (ip net.IP, err error) {
	if len(gw.Status.Addresses) > 1 {
		return nil, fmt.Errorf("Gateway %s/%s had %d addresses but we only currently support 1", gw.Namespace, gw.Name, len(gw.Status.Addresses))
	}

	for _, address := range gw.Status.Addresses {
		if address.Type != nil && *address.Type == gatewayv1beta1.IPAddressType {
			ip = net.ParseIP(address.Value)
			return
		}
	}

	err = fmt.Errorf("IP address not ready for Gateway %s/%s", gw.Namespace, gw.Name)
	return
}

func getGatewayPort(gw *gatewayv1beta1.Gateway, refs []gatewayv1alpha2.ParentReference) (uint32, error) {
	if len(refs) > 1 {
		// TODO: https://github.com/Kong/blixt/issues/10
		return 0, fmt.Errorf("multiple parentRefs not yet supported")
	}

	if refs[0].Port == nil {
		return 0, fmt.Errorf("port not found for parentRef")
	}

	return uint32(*refs[0].Port), nil
}
