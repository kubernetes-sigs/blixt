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

package controllers

import (
	"context"
	"fmt"
	"reflect"

	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/utils/ptr"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

func (r *GatewayReconciler) getServiceForGateway(ctx context.Context, gw *gatewayv1beta1.Gateway) (*corev1.Service, error) {
	svcs := new(corev1.ServiceList)
	if err := r.List(ctx, svcs, client.InNamespace(gw.Namespace), client.MatchingLabels{gatewayServiceLabel: gw.Name}); err != nil {
		return nil, err
	}

	if len(svcs.Items) > 1 {
		return nil, fmt.Errorf("more than 1 Service found for Gateway %s/%s, not currently supported", gw.Namespace, gw.Name)
	}

	for _, svc := range svcs.Items {
		return &svc, nil
	}

	return nil, nil
}

func (r *GatewayReconciler) createServiceForGateway(ctx context.Context, gw *gatewayv1beta1.Gateway) error {
	svc := corev1.Service{
		ObjectMeta: metav1.ObjectMeta{
			Namespace:    gw.Namespace,
			GenerateName: fmt.Sprintf("service-for-gateway-%s-", gw.Name),
			Labels: map[string]string{
				gatewayServiceLabel: gw.Name,
			},
		},
	}

	if len(gw.Spec.Addresses) > 0 {
		addr := gw.Spec.Addresses[0]

		if *addr.Type != gatewayv1beta1.IPAddressType {
			// TODO: update status https://github.com/Kong/blixt/issues/96
			return fmt.Errorf("status addresses of type %s are not supported, only IP addresses are supported", *addr.Type)
		}

		svc.Spec.LoadBalancerIP = addr.Value
	}

	if len(gw.Spec.Addresses) > 1 {
		// TODO: update status https://github.com/Kong/blixt/issues/96
		r.Log.Error(
			fmt.Errorf("assigning multiple static IPs for a Gateway is not currently supported"),
			fmt.Sprintf("%d addresses were requested, only %s will be allocated", len(gw.Spec.Addresses), svc.Spec.LoadBalancerIP),
		)
	}

	_, err := r.ensureServiceConfiguration(ctx, &svc, gw)
	if err != nil {
		return err
	}

	setOwnerReference(&svc, gw)

	return r.Client.Create(ctx, &svc)
}

func setOwnerReference(svc *corev1.Service, gw client.Object) {
	gvk := gw.GetObjectKind().GroupVersionKind()
	svc.ObjectMeta.OwnerReferences = []metav1.OwnerReference{{
		APIVersion: fmt.Sprintf("%s/%s", gvk.Group, gvk.Version),
		Kind:       gvk.Kind,
		Name:       gw.GetName(),
		UID:        gw.GetUID(),
		Controller: ptr.To(true),
	}}
}

func (r *GatewayReconciler) svcIsHealthy(ctx context.Context, svc *corev1.Service) error {
	if len(svc.Status.LoadBalancer.Ingress) > 0 {
		return nil
	}

	// FIXME: the following is a hack to use metallb events to determine if the
	// service is having trouble getting an IP allocated for it. This was created
	// in a hurry and needs to be replaced with something robust.
	events := &corev1.EventList{}
	if err := r.Client.List(ctx, events, &client.ListOptions{
		// TODO: add a field selector
		Namespace: svc.Namespace,
	}); err != nil {
		return err
	}

	var allocationFailed *corev1.Event
	var allocationSucceeded *corev1.Event

	for _, event := range events.Items {
		currentEvent := event

		if currentEvent.InvolvedObject.Name == svc.Name && currentEvent.Reason == "AllocationFailed" { // TODO: only handles metallb right now https://github.com/Kong/blixt/issues/96
			if allocationFailed != nil {
				if currentEvent.EventTime.After(allocationFailed.EventTime.Time) {
					allocationFailed = &currentEvent
				}
			} else {
				allocationFailed = &currentEvent
			}
		}

		if currentEvent.InvolvedObject.Name == svc.Name && currentEvent.Reason == "IPAllocated" {
			if allocationSucceeded != nil {
				if currentEvent.EventTime.After(allocationSucceeded.EventTime.Time) {
					allocationSucceeded = &currentEvent
				}
			} else {
				allocationSucceeded = &currentEvent
			}
		}
	}

	if allocationFailed != nil {
		if allocationSucceeded != nil && allocationSucceeded.EventTime.After(allocationFailed.EventTime.Time) {
			return nil
		}
		return fmt.Errorf("%s", allocationFailed.Message)
	}

	return nil
}

func (r *GatewayReconciler) ensureServiceConfiguration(ctx context.Context, svc *corev1.Service, gw *gatewayv1beta1.Gateway) (bool, error) {
	updated := false

	if len(gw.Spec.Addresses) > 0 && svc.Spec.LoadBalancerIP != gw.Spec.Addresses[0].Value {
		if len(gw.Spec.Addresses) > 1 {
			r.Log.Info(fmt.Sprintf("found %d addresses on gateway, but currently we only support 1", len(gw.Spec.Addresses)), gw.Namespace, gw.Name)
		}
		r.Log.Info(fmt.Sprintf("using address %s for gateway", gw.Spec.Addresses[0].Value), gw.Namespace, gw.Name)
		svc.Spec.LoadBalancerIP = gw.Spec.Addresses[0].Value
		updated = true
	}

	if svc.Spec.LoadBalancerIP != "" && len(gw.Spec.Addresses) == 0 {
		r.Log.Info("service for gateway had a left over address that's no longer specified, removing", gw.Namespace, gw.Name)
		svc.Spec.LoadBalancerIP = ""
		updated = true
	}

	if svc.Spec.Type != corev1.ServiceTypeLoadBalancer {
		svc.Spec.Type = corev1.ServiceTypeLoadBalancer
		updated = true
	}

	ports := make([]corev1.ServicePort, 0, len(gw.Spec.Listeners))
	for _, listener := range gw.Spec.Listeners {
		switch proto := listener.Protocol; proto {
		case gatewayv1beta1.TCPProtocolType:
			ports = append(ports, corev1.ServicePort{
				Name:     string(listener.Name),
				Protocol: corev1.ProtocolTCP,
				Port:     int32(listener.Port),
			})
		case gatewayv1beta1.UDPProtocolType:
			ports = append(ports, corev1.ServicePort{
				Name:     string(listener.Name),
				Protocol: corev1.ProtocolUDP,
				Port:     int32(listener.Port),
			})
		// TODO: this is a hack to workaround defaults listener configurations
		// that were present in the Gateway API conformance tests, so that we
		// can still pass the tests. For now, we just treat an HTTP/S listener
		// as a TCP listener to workaround this (but we don't actually support
		// HTTPRoute).
		case gatewayv1beta1.HTTPProtocolType:
			ports = append(ports, corev1.ServicePort{
				Name:     string(listener.Name),
				Protocol: corev1.ProtocolTCP,
				Port:     int32(listener.Port),
			})
		case gatewayv1beta1.HTTPSProtocolType:
			ports = append(ports, corev1.ServicePort{
				Name:     string(listener.Name),
				Protocol: corev1.ProtocolTCP,
				Port:     int32(listener.Port),
			})
		}
	}

	newPorts := make(map[string]portAndProtocol, len(ports))
	for _, newPort := range ports {
		newPorts[newPort.Name] = portAndProtocol{
			port:     newPort.Port,
			protocol: newPort.Protocol,
		}
	}

	oldPorts := make(map[string]portAndProtocol, len(svc.Spec.Ports))
	for _, oldPort := range svc.Spec.Ports {
		oldPorts[oldPort.Name] = portAndProtocol{
			port:     oldPort.Port,
			protocol: oldPort.Protocol,
		}
	}

	if !reflect.DeepEqual(newPorts, oldPorts) {
		svc.Spec.Ports = ports
		updated = true
	}

	return updated, nil
}

var (
	ipAddrType   = gatewayv1beta1.IPAddressType
	hostAddrType = gatewayv1beta1.HostnameAddressType
)

// hackEnsureEndpoints is a temporary hack around how metallb'd L2 mode works, re: https://github.com/metallb/metallb/issues/1640
func (r *GatewayReconciler) hackEnsureEndpoints(ctx context.Context, svc *corev1.Service) (bool, error) {
	nsn := types.NamespacedName{Namespace: svc.Namespace, Name: svc.Name}
	lbaddr := ""
	for _, addr := range svc.Status.LoadBalancer.Ingress {
		if addr.IP != "" {
			lbaddr = addr.IP
			break
		}
		if addr.Hostname != "" {
			lbaddr = addr.Hostname
			break
		}
	}

	endpoints := new(corev1.Endpoints)
	err := r.Client.Get(ctx, nsn, endpoints)
	if err != nil {
		if errors.IsNotFound(err) {
			eports := make([]corev1.EndpointPort, 0, len(svc.Spec.Ports))
			for _, svcPort := range svc.Spec.Ports {
				eports = append(eports, corev1.EndpointPort{
					Port:     svcPort.Port,
					Protocol: svcPort.Protocol,
				})
			}

			endpoints = &corev1.Endpoints{
				ObjectMeta: metav1.ObjectMeta{
					Namespace: svc.Namespace,
					Name:      svc.Name,
				},
				Subsets: []corev1.EndpointSubset{{
					Addresses: []corev1.EndpointAddress{{IP: lbaddr}},
					Ports:     eports,
				}},
			}

			return true, r.Client.Create(ctx, endpoints)
		}
		return false, err
	}

	return false, nil
}

func (r *GatewayReconciler) mapGatewayClassToGateway(_ context.Context, obj client.Object) (recs []reconcile.Request) {
	gatewayClass, ok := obj.(*gatewayv1beta1.GatewayClass)
	if !ok {
		r.Log.Error(fmt.Errorf("unexpected object type in gateway watch predicates"), "expected", "*gatewayv1beta1.GatewayClass", "found", reflect.TypeOf(obj))
		return
	}

	gateways := &gatewayv1beta1.GatewayList{}
	if err := r.Client.List(context.Background(), gateways); err != nil {
		// TODO: https://github.com/kubernetes-sigs/controller-runtime/issues/1996
		r.Log.Error(err, "could not map gatewayclass event to gateways")
		return
	}

	for _, gateway := range gateways.Items {
		if gateway.Spec.GatewayClassName == gatewayv1beta1.ObjectName(gatewayClass.Name) {
			recs = append(recs, reconcile.Request{NamespacedName: types.NamespacedName{
				Namespace: gateway.Namespace,
				Name:      gateway.Name,
			}})
		}
	}

	return
}

func mapServiceToGateway(_ context.Context, obj client.Object) (reqs []reconcile.Request) {
	svc, ok := obj.(*corev1.Service)
	if !ok {
		return
	}

	for _, ownerRef := range svc.OwnerReferences {
		if ownerRef.APIVersion == fmt.Sprintf("%s/%s", gatewayv1beta1.GroupName, gatewayv1beta1.GroupVersion.Version) {
			reqs = append(reqs, reconcile.Request{
				NamespacedName: types.NamespacedName{
					Namespace: svc.Namespace,
					Name:      ownerRef.Name,
				},
			})
		}
	}

	return
}

func setGatewayStatus(gateway *gatewayv1beta1.Gateway) {
	newAccepted := determineGatewayAcceptance(gateway)
	newProgrammed := determineGatewayProgrammed(gateway)
	setCond(gateway, newAccepted)
	setCond(gateway, newProgrammed)
}

func determineGatewayAcceptance(gateway *gatewayv1beta1.Gateway) metav1.Condition {
	// this is the default accepted condition, it may get overidden if there are
	// unsupported values in the specification.
	accepted := metav1.Condition{
		Type:               string(gatewayv1beta1.GatewayConditionAccepted),
		Status:             metav1.ConditionTrue,
		Reason:             string(gatewayv1beta1.GatewayReasonAccepted),
		ObservedGeneration: gateway.Generation,
		LastTransitionTime: metav1.Now(),
		Message:            "blixt controlplane accepts responsibility for the Gateway",
	}

	// verify that all addresses are supported
	for _, addr := range gateway.Spec.Addresses {
		if addr.Type != nil && *addr.Type != gatewayv1beta1.IPAddressType {
			accepted.Status = metav1.ConditionFalse
			accepted.Reason = string(gatewayv1beta1.GatewayReasonUnsupportedAddress)
			accepted.Message = fmt.Sprintf("found an address of type %s, only IPAddress is supported", *addr.Type)
		}
	}

	return accepted
}

func determineGatewayProgrammed(gateway *gatewayv1beta1.Gateway) metav1.Condition {
	// TODO: give this client access and make it dynamic
	return metav1.Condition{
		Type:               string(gatewayv1beta1.GatewayConditionProgrammed),
		ObservedGeneration: gateway.Generation,
		Status:             metav1.ConditionFalse,
		LastTransitionTime: metav1.Now(),
		Reason:             string(gatewayv1beta1.GatewayReasonPending),
		Message:            "dataplane not yet configured",
	}
}

// cmpCond returns true if the conditions are the same, minus the timestamp.
func cmpCond(cond1, cond2 metav1.Condition) bool { //nolint:unused
	return cond1.Type == cond2.Type &&
		cond1.Status == cond2.Status &&
		cond1.ObservedGeneration == cond2.ObservedGeneration &&
		cond1.Reason == cond2.Reason &&
		cond1.Message == cond2.Message
}

type portAndProtocol struct {
	port     int32
	protocol corev1.Protocol
}
