package controllers

import (
	"context"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

func markGatewayReady(ctx context.Context, gateway *gatewayv1beta1.Gateway, svc *corev1.Service) {
	gwaddrs := make([]gatewayv1beta1.GatewayAddress, 0, len(svc.Status.LoadBalancer.Ingress))
	for _, addr := range svc.Status.LoadBalancer.Ingress {
		if addr.IP != "" {
			gwaddrs = append(gwaddrs, gatewayv1beta1.GatewayAddress{
				Type:  &ipAddrType,
				Value: addr.IP,
			})
		}
		if addr.Hostname != "" {
			gwaddrs = append(gwaddrs, gatewayv1beta1.GatewayAddress{
				Type:  &hostAddrType,
				Value: addr.Hostname,
			})
		}
	}
	gateway.Status.Addresses = gwaddrs
	gateway.Status.Conditions = []metav1.Condition{{
		Type:               string(gatewayv1beta1.GatewayConditionReady),
		Status:             metav1.ConditionTrue,
		Reason:             string(gatewayv1beta1.GatewayReasonReady),
		ObservedGeneration: gateway.Generation,
		LastTransitionTime: metav1.Now(),
		Message:            "the gateway is ready to route traffic",
	}}

	listenersStatus := make([]gatewayv1beta1.ListenerStatus, 0, len(gateway.Spec.Listeners))
	for _, l := range gateway.Spec.Listeners {
		supportedKinds, resolvedRefsCondition := getSupportedKinds(gateway.Generation, l)
		listenersStatus = append(listenersStatus, gatewayv1beta1.ListenerStatus{
			Name:           l.Name,
			SupportedKinds: supportedKinds,
			Conditions: []metav1.Condition{
				{
					Type:               string(gatewayv1beta1.ListenerConditionReady),
					Status:             metav1.ConditionTrue,
					Reason:             string(gatewayv1beta1.ListenerReasonReady),
					ObservedGeneration: gateway.Generation,
					LastTransitionTime: metav1.Now(),
					Message:            "the listener is ready to route traffic",
				},
				resolvedRefsCondition,
			},
		})
	}
	gateway.Status.Listeners = listenersStatus
}

func initGatewayStatus(gateway *gatewayv1beta1.Gateway) {
	gateway.Status = gatewayv1beta1.GatewayStatus{
		Conditions: []metav1.Condition{
			{
				Type:               string(gatewayv1beta1.GatewayConditionReady),
				Status:             metav1.ConditionFalse,
				Reason:             string(gatewayv1beta1.GatewayReasonReady),
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
					Type:               string(gatewayv1beta1.ListenerConditionReady),
					Status:             metav1.ConditionFalse,
					Reason:             string(gatewayv1beta1.ListenerReasonPending),
					ObservedGeneration: gateway.Generation,
					LastTransitionTime: metav1.Now(),
					Message:            "the listener is not ready to route traffic",
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

func (r *GatewayReconciler) patchGatewayStatus(ctx context.Context, gateway, oldGateway *gatewayv1beta1.Gateway) error {
	return r.Status().Patch(ctx, gateway, client.MergeFrom(oldGateway))
}
