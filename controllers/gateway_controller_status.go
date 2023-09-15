package controllers

import (
	"context"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

// updateGatewayStatus computes the new Gateway status, setting its ready condition and all the
// ready listeners's ready conditions to true, unless a resolvedRefs error is discovered. In
// that case, the proper listener ready condition and the gateway one are set to false.
// The addresses are updated as well.
func updateGatewayStatus(_ context.Context, gateway *gatewayv1beta1.Gateway, svc *corev1.Service) {
	// gateway addresses
	gwaddrs := make([]gatewayv1beta1.GatewayStatusAddress, 0, len(svc.Status.LoadBalancer.Ingress))
	for _, addr := range svc.Status.LoadBalancer.Ingress {
		if addr.IP != "" {
			gwaddrs = append(gwaddrs, gatewayv1beta1.GatewayStatusAddress{
				Type:  &ipAddrType,
				Value: addr.IP,
			})
		}
		if addr.Hostname != "" {
			gwaddrs = append(gwaddrs, gatewayv1beta1.GatewayStatusAddress{
				Type:  &hostAddrType,
				Value: addr.Hostname,
			})
		}
	}
	gateway.Status.Addresses = gwaddrs

	// gateway conditions
	newGatewayProgrammedCondition := metav1.Condition{
		Type:               string(gatewayv1beta1.GatewayConditionProgrammed),
		Status:             metav1.ConditionTrue,
		Reason:             string(gatewayv1beta1.GatewayReasonProgrammed),
		ObservedGeneration: gateway.Generation,
		LastTransitionTime: metav1.Now(),
		Message:            "the gateway is ready to route traffic",
	}

	// gateway listeners conditions
	listenersStatus := make([]gatewayv1beta1.ListenerStatus, 0, len(gateway.Spec.Listeners))
	for _, l := range gateway.Spec.Listeners {
		supportedKinds, resolvedRefsCondition := getSupportedKinds(gateway.Generation, l)
		listenerProgrammedStatus := corev1.ConditionTrue
		listenerProgrammedReason := gatewayv1beta1.ListenerReasonProgrammed
		if resolvedRefsCondition.Status == metav1.ConditionFalse {
			listenerProgrammedStatus = corev1.ConditionStatus(metav1.ConditionFalse)
			listenerProgrammedReason = gatewayv1beta1.ListenerReasonResolvedRefs
		}
		listenersStatus = append(listenersStatus, gatewayv1beta1.ListenerStatus{
			Name:           l.Name,
			SupportedKinds: supportedKinds,
			Conditions: []metav1.Condition{
				{
					Type:               string(gatewayv1beta1.ListenerConditionProgrammed),
					Status:             metav1.ConditionStatus(listenerProgrammedStatus),
					Reason:             string(listenerProgrammedReason),
					ObservedGeneration: gateway.Generation,
					LastTransitionTime: metav1.Now(),
				},
				resolvedRefsCondition,
			},
		})
		if resolvedRefsCondition.Status == metav1.ConditionFalse {
			newGatewayProgrammedCondition.Status = metav1.ConditionFalse
			newGatewayProgrammedCondition.Reason = string(gatewayv1beta1.GatewayReasonAddressNotAssigned)
			newGatewayProgrammedCondition.Message = "the gateway is not ready to route traffic"
		}
	}

	gateway.Status.Conditions = []metav1.Condition{
		newGatewayProgrammedCondition,
	}
	gateway.Status.Listeners = listenersStatus
}

// initGatewayStatus initializes the GatewayStatus, setting the ready condition to
// not ready and all the listeners ready status to not ready as well.
func initGatewayStatus(gateway *gatewayv1beta1.Gateway) {
	gateway.Status = gatewayv1beta1.GatewayStatus{
		Conditions: []metav1.Condition{
			{
				Type:               string(gatewayv1beta1.GatewayConditionProgrammed),
				Status:             metav1.ConditionFalse,
				Reason:             string(gatewayv1beta1.GatewayReasonAddressNotAssigned),
				ObservedGeneration: gateway.Generation,
				LastTransitionTime: metav1.Now(),
				Message:            "the gateway is not ready to route traffic",
			},
		},
	}
	gateway.Status.Listeners = make([]gatewayv1beta1.ListenerStatus, 0, len(gateway.Spec.Listeners))
	for _, l := range gateway.Spec.Listeners {
		supportedKinds, resolvedRefsCondition := getSupportedKinds(gateway.Generation, l)
		gateway.Status.Listeners = append(gateway.Status.Listeners, gatewayv1beta1.ListenerStatus{
			Name:           l.Name,
			SupportedKinds: supportedKinds,
			Conditions: []metav1.Condition{
				{
					Type:               string(gatewayv1beta1.ListenerConditionProgrammed),
					Status:             metav1.ConditionFalse,
					Reason:             string(gatewayv1beta1.ListenerReasonPending),
					ObservedGeneration: gateway.Generation,
					LastTransitionTime: metav1.Now(),
				},
				resolvedRefsCondition,
			},
		})
	}
}

func getSupportedKinds(generation int64, listener gatewayv1beta1.Listener) (supportedKinds []gatewayv1beta1.RouteGroupKind, resolvedRefsCondition metav1.Condition) {
	supportedKinds = make([]gatewayv1beta1.RouteGroupKind, 0)
	resolvedRefsCondition = metav1.Condition{
		Type:               string(gatewayv1beta1.ListenerConditionResolvedRefs),
		Status:             metav1.ConditionTrue,
		Reason:             string(gatewayv1beta1.ListenerReasonResolvedRefs),
		ObservedGeneration: generation,
		LastTransitionTime: metav1.Now(),
	}
	if len(listener.AllowedRoutes.Kinds) == 0 {
		// When unspecified or empty, the kinds of Routes selected are determined using the Listener protocol.
		switch listener.Protocol {
		case gatewayv1beta1.TCPProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1beta1.RouteGroupKind{
				Group: (*gatewayv1beta1.Group)(&gatewayv1beta1.GroupVersion.Group),
				Kind:  "TCPRoute",
			})
		case gatewayv1beta1.UDPProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1beta1.RouteGroupKind{
				Group: (*gatewayv1beta1.Group)(&gatewayv1beta1.GroupVersion.Group),
				Kind:  "UDPRoute",
			})
		// TODO: this is a hack to workaround defaults listener configurations
		// that were present in the Gateway API conformance tests, so that we
		// can still pass the tests. For now, we just treat an HTTP/S listener
		// as a TCP listener to workaround this (but we don't actually support
		// HTTPRoute).
		case gatewayv1beta1.HTTPProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1beta1.RouteGroupKind{
				Group: (*gatewayv1beta1.Group)(&gatewayv1beta1.GroupVersion.Group),
				Kind:  "TCPRoute",
			})
		case gatewayv1beta1.HTTPSProtocolType:
			supportedKinds = append(supportedKinds, gatewayv1beta1.RouteGroupKind{
				Group: (*gatewayv1beta1.Group)(&gatewayv1beta1.GroupVersion.Group),
				Kind:  "TCPRoute",
			})
		default:
			resolvedRefsCondition.Status = metav1.ConditionFalse
			resolvedRefsCondition.Reason = string(gatewayv1beta1.ListenerReasonInvalidRouteKinds)
		}
	}

	for _, k := range listener.AllowedRoutes.Kinds {
		if (k.Group != nil && *k.Group != "" && *k.Group != gatewayv1beta1.Group(gatewayv1beta1.GroupVersion.Group)) ||
			(k.Kind != "UDPRoute" && k.Kind != "TCPRoute") {
			resolvedRefsCondition.Status = metav1.ConditionFalse
			resolvedRefsCondition.Reason = string(gatewayv1beta1.ListenerReasonInvalidRouteKinds)
			continue
		}
		supportedKinds = append(supportedKinds, gatewayv1beta1.RouteGroupKind{
			Group: k.Group,
			Kind:  k.Kind,
		})
	}
	return supportedKinds, resolvedRefsCondition
}

// factorizeStatus takes the old gateway conditions not transitioned and copies them
// into the new gateway status, so that only the transitioning conditions gets actually patched.
func factorizeStatus(gateway, oldGateway *gatewayv1beta1.Gateway) {
	for i, c := range gateway.Status.Conditions {
		for _, oldC := range oldGateway.Status.Conditions {
			if c.Type == oldC.Type {
				if c.Status == oldC.Status && c.Reason == oldC.Reason {
					gateway.Status.Conditions[i] = oldC
				}
			}
		}
	}

	for i, l := range gateway.Status.Listeners {
		for j, lc := range l.Conditions {
			for _, ol := range oldGateway.Status.Listeners {
				if ol.Name != l.Name {
					continue
				}
				for _, olc := range ol.Conditions {
					if lc.Type == olc.Type {
						if lc.Status == olc.Status && lc.Reason == olc.Reason {
							gateway.Status.Listeners[i].Conditions[j] = olc
						}
					}
				}
			}
		}
	}
}

// isGatewayProgrammed returns two boolean values:
// - the status of the programmed condition
// - a boolean flag to check if the condition exists
func isGatewayProgrammed(gateway *gatewayv1beta1.Gateway) (status bool, isSet bool) {
	for _, c := range gateway.Status.Conditions {
		if c.Type == string(gatewayv1beta1.GatewayConditionProgrammed) {
			return c.Status == metav1.ConditionTrue, true
		}
	}
	return false, false
}
