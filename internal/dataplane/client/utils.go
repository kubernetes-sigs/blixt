/*
Copyright 2023 The Kubernetes Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

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

	gatewayIP, err := GetGatewayIP(gateway)
	if gatewayIP == nil {
		return nil, err
	}

	gatewayPort, err := GetGatewayPort(gateway, udproute.Spec.ParentRefs)
	if err != nil {
		return nil, err
	}
	var backendTargets []*Target
	for _, rule := range udproute.Spec.Rules {
		for _, backendRef := range rule.BackendRefs {
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

				for _, addr := range subset.Addresses {
					if addr.IP == "" {
						return nil, fmt.Errorf("empty IP for endpoint subset")
					}

					ip := net.ParseIP(addr.IP)
					podip := binary.BigEndian.Uint32(ip.To4())
					podPort, err := getBackendPort(ctx, c, udproute.Namespace, backendRef, subset.Ports)
					if err != nil {
						return nil, err
					}

					target := &Target{
						Daddr: podip,
						Dport: uint32(podPort),
					}
					backendTargets = append(backendTargets, target)
				}
			}
		}
	}

	if len(backendTargets) == 0 {
		return nil, fmt.Errorf("no healthy backends")
	}

	ipint := binary.BigEndian.Uint32(gatewayIP.To4())

	targets := &Targets{
		Vip: &Vip{
			Ip:   ipint,
			Port: gatewayPort,
		},
		Targets: backendTargets,
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

	gatewayIP, err := GetGatewayIP(gateway)
	if gatewayIP == nil {
		return nil, err
	}

	gatewayPort, err := GetGatewayPort(gateway, tcproute.Spec.ParentRefs)
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
			podPort, err := getBackendPort(ctx, c, tcproute.Namespace, backendRef, subset.Ports)
			if err != nil {
				return nil, err
			}

			target = &Target{
				Daddr: podip,
				Dport: uint32(podPort),
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
		// TODO(aryan9600): Add support for multiple targets (https://github.com/kubernetes-sigs/blixt/issues/119)
		Targets: []*Target{target},
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

func getBackendPort(ctx context.Context, c client.Client, ns string, backendRef gatewayv1alpha2.BackendRef,
	epPorts []corev1.EndpointPort) (int32, error) {
	svc := new(corev1.Service)
	if backendRef.Namespace != nil {
		ns = string(*backendRef.Namespace)
	}
	key := client.ObjectKey{
		Namespace: ns,
		Name:      string(backendRef.Name),
	}
	if err := c.Get(ctx, key, svc); err != nil {
		return 0, err
	}

	for _, port := range svc.Spec.Ports {
		// backendRef must have a port if the backend is a Service.
		if port.Port == int32(*backendRef.Port) {
			if port.TargetPort.IntValue() == 0 {
				return port.Port, nil
			}
			return int32(port.TargetPort.IntValue()), nil
		}
	}
	return 0, fmt.Errorf("could not find target port for backend ref: %s", key.String())
}

func GetGatewayIP(gw *gatewayv1beta1.Gateway) (ip net.IP, err error) {
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

func GetGatewayPort(gw *gatewayv1beta1.Gateway, refs []gatewayv1alpha2.ParentReference) (uint32, error) {
	if len(refs) > 1 {
		// TODO: https://github.com/Kong/blixt/issues/10
		return 0, fmt.Errorf("multiple parentRefs not yet supported")
	}

	if refs[0].Port == nil {
		return 0, fmt.Errorf("port not found for parentRef")
	}

	return uint32(*refs[0].Port), nil
}
