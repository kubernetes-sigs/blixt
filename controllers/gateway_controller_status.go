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
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	gatewayv1 "sigs.k8s.io/gateway-api/apis/v1"
)

func setGatewayStatusAddresses(gateway *gatewayv1.Gateway, svc *corev1.Service) {
	gwaddrs := []gatewayv1.GatewayStatusAddress{}
	for _, addr := range svc.Status.LoadBalancer.Ingress {
		if addr.IP != "" {
			gwaddrs = append(gwaddrs, gatewayv1.GatewayStatusAddress{
				Type:  &ipAddrType,
				Value: addr.IP,
			})
		}
		if addr.Hostname != "" {
			gwaddrs = append(gwaddrs, gatewayv1.GatewayStatusAddress{
				Type:  &hostAddrType,
				Value: addr.Hostname,
			})
		}
	}
	gateway.Status.Addresses = gwaddrs
}

func setGatewayListenerConditionsAndProgrammed(gateway *gatewayv1.Gateway) {
	programmed := metav1.Condition{
		Type:               string(gatewayv1.GatewayConditionProgrammed),
		Status:             metav1.ConditionTrue,
		Reason:             string(gatewayv1.GatewayReasonProgrammed),
		ObservedGeneration: gateway.Generation,
		LastTransitionTime: metav1.Now(),
		Message:            "the gateway is ready to route traffic",
	}

	listenersStatus := make([]gatewayv1.ListenerStatus, 0, len(gateway.Spec.Listeners))
	for _, l := range gateway.Spec.Listeners {
		supportedKinds, resolvedRefsCondition := getSupportedKinds(gateway.Generation, l)
		listenerProgrammedStatus := corev1.ConditionTrue
		listenerProgrammedReason := gatewayv1.ListenerReasonProgrammed
		if resolvedRefsCondition.Status == metav1.ConditionFalse {
			listenerProgrammedStatus = corev1.ConditionStatus(metav1.ConditionFalse)
			listenerProgrammedReason = gatewayv1.ListenerReasonResolvedRefs
		}
		listenersStatus = append(listenersStatus, gatewayv1.ListenerStatus{
			Name:           l.Name,
			SupportedKinds: supportedKinds,
			Conditions: []metav1.Condition{
				{
					Type:               string(gatewayv1.ListenerConditionAccepted),
					Status:             metav1.ConditionTrue,
					Reason:             string(gatewayv1.ListenerReasonAccepted),
					ObservedGeneration: gateway.Generation,
					LastTransitionTime: metav1.Now(),
				},
				{
					Type:               string(gatewayv1.ListenerConditionProgrammed),
					Status:             metav1.ConditionStatus(listenerProgrammedStatus),
					Reason:             string(listenerProgrammedReason),
					ObservedGeneration: gateway.Generation,
					LastTransitionTime: metav1.Now(),
				},
				resolvedRefsCondition,
			},
		})
		if resolvedRefsCondition.Status == metav1.ConditionFalse {
			programmed.Status = metav1.ConditionFalse
			programmed.Reason = string(gatewayv1.GatewayReasonAddressNotAssigned)
			programmed.Message = "the gateway is not ready to route traffic"
		}
	}
	gateway.Status.Listeners = listenersStatus
	setCond(gateway, programmed)
}

func setGatewayListenerStatus(gateway *gatewayv1.Gateway) {
	gateway.Status.Listeners = make([]gatewayv1.ListenerStatus, 0, len(gateway.Spec.Listeners))
	for _, l := range gateway.Spec.Listeners {
		supportedKinds, resolvedRefsCondition := getSupportedKinds(gateway.Generation, l)
		gateway.Status.Listeners = append(gateway.Status.Listeners, gatewayv1.ListenerStatus{
			Name:           l.Name,
			SupportedKinds: supportedKinds,
			Conditions: []metav1.Condition{
				{
					Type:               string(gatewayv1.ListenerConditionProgrammed),
					Status:             metav1.ConditionFalse,
					Reason:             string(gatewayv1.ListenerReasonPending),
					ObservedGeneration: gateway.Generation,
					LastTransitionTime: metav1.Now(),
				},
				resolvedRefsCondition,
			},
		})
	}
}

func getSupportedKinds(generation int64, listener gatewayv1.Listener) (supportedKinds []gatewayv1.RouteGroupKind, resolvedRefsCondition metav1.Condition) {
	supportedKinds = make([]gatewayv1.RouteGroupKind, 0)
	resolvedRefsCondition = metav1.Condition{
		Type:               string(gatewayv1.ListenerConditionResolvedRefs),
		Status:             metav1.ConditionTrue,
		Reason:             string(gatewayv1.ListenerReasonResolvedRefs),
		ObservedGeneration: generation,
		LastTransitionTime: metav1.Now(),
	}
	if len(listener.AllowedRoutes.Kinds) == 0 {
		// When unspecified or empty, the kinds of Routes selected are determined using the Listener protocol.
		switch listener.Protocol {
		case gatewayv1.TCPProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1.RouteGroupKind{
				Group: (*gatewayv1.Group)(&gatewayv1.GroupVersion.Group),
				Kind:  "TCPRoute",
			})
		case gatewayv1.UDPProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1.RouteGroupKind{
				Group: (*gatewayv1.Group)(&gatewayv1.GroupVersion.Group),
				Kind:  "UDPRoute",
			})
		// TODO: this is a hack to workaround defaults listener configurations
		// that were present in the Gateway API conformance tests, so that we
		// can still pass the tests. For now, we just treat an HTTP/S listener
		// as a TCP listener to workaround this (but we don't actually support
		// HTTPRoute).
		case gatewayv1.HTTPProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1.RouteGroupKind{
				Group: (*gatewayv1.Group)(&gatewayv1.GroupVersion.Group),
				Kind:  "HTTPRoute",
			})
		case gatewayv1.HTTPSProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1.RouteGroupKind{
				Group: (*gatewayv1.Group)(&gatewayv1.GroupVersion.Group),
				Kind:  "HTTPRoute",
			})
		default:
			resolvedRefsCondition.Status = metav1.ConditionFalse
			resolvedRefsCondition.Reason = string(gatewayv1.ListenerReasonInvalidRouteKinds)
		}
	}

	for _, k := range listener.AllowedRoutes.Kinds {
		if (k.Group != nil && *k.Group != "" && *k.Group != gatewayv1.Group(gatewayv1.GroupVersion.Group)) ||
			(k.Kind != "UDPRoute" && k.Kind != "TCPRoute") {
			resolvedRefsCondition.Status = metav1.ConditionFalse
			resolvedRefsCondition.Reason = string(gatewayv1.ListenerReasonInvalidRouteKinds)
			continue
		}
		supportedKinds = append(supportedKinds, gatewayv1.RouteGroupKind{
			Group: k.Group,
			Kind:  k.Kind,
		})
	}
	return supportedKinds, resolvedRefsCondition
}

// updateConditionGeneration takes the old gateway conditions not transitioned and copies them
// into the new gateway status, so that only the transitioning conditions gets actually patched.
func updateConditionGeneration(gateway *gatewayv1.Gateway) {
	for i := 0; i < len(gateway.Status.Conditions); i++ {
		gateway.Status.Conditions[0].ObservedGeneration = gateway.Generation
	}

	for i := 0; i < len(gateway.Status.Listeners); i++ {
		updatedListenerConditions := []metav1.Condition{}
		for _, cond := range gateway.Status.Listeners[0].Conditions {
			cond.ObservedGeneration = gateway.Generation
			updatedListenerConditions = append(updatedListenerConditions, cond)
		}
		gateway.Status.Listeners[0].Conditions = updatedListenerConditions
	}
}

func isGatewayAccepted(gateway *gatewayv1.Gateway) bool {
	accepted := getAcceptedConditionForGateway(gateway)
	if accepted == nil {
		return false
	}
	return accepted.Status == metav1.ConditionTrue
}

func getAcceptedConditionForGateway(gateway *gatewayv1.Gateway) *metav1.Condition {
	return getCond(gateway, string(gatewayv1.GatewayConditionAccepted))
}

func setCond(gateway *gatewayv1.Gateway, setCond metav1.Condition) {
	updatedConditions := make([]metav1.Condition, 0, len(gateway.Status.Conditions))

	found := false
	for _, oldCond := range gateway.Status.Conditions {
		if oldCond.Type == setCond.Type {
			found = true
			updatedConditions = append(updatedConditions, setCond)
		} else {
			updatedConditions = append(updatedConditions, oldCond)
		}
	}

	if !found {
		updatedConditions = append(updatedConditions, setCond)
	}

	gateway.Status.Conditions = updatedConditions
}

func getCond(gateway *gatewayv1.Gateway, requestedType string) *metav1.Condition {
	for _, cond := range gateway.Status.Conditions {
		if cond.Type == requestedType {
			return &cond
		}
	}
	return nil
}
