package controllers

import (
	"context"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

func (r *GatewayReconciler) markGatewayReady(ctx context.Context, gw *gatewayv1beta1.Gateway, svc *corev1.Service) error {
	previousGW := gw.DeepCopy()

	for _, cond := range gw.Status.Conditions {
		if cond.Type == string(gatewayv1beta1.GatewayConditionReady) && cond.Status == metav1.ConditionTrue {
			return nil
		}
	}

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
	gw.Status.Addresses = gwaddrs
	gw.Status.Conditions = []metav1.Condition{{
		Type:               string(gatewayv1beta1.GatewayConditionReady),
		Status:             metav1.ConditionTrue,
		Reason:             string(gatewayv1beta1.GatewayReasonReady),
		ObservedGeneration: gw.Generation,
		LastTransitionTime: metav1.Now(),
		Message:            "the gateway is ready to route traffic",
	}}

	listenersStatus := make([]gatewayv1beta1.ListenerStatus, 0, len(gw.Spec.Listeners))
	for _, l := range gw.Spec.Listeners {
		listenersStatus = append(listenersStatus, gatewayv1beta1.ListenerStatus{
			Name: l.Name,
			Conditions: []metav1.Condition{
				{
					Type:               string(gatewayv1beta1.ListenerConditionReady),
					Status:             metav1.ConditionTrue,
					Reason:             string(gatewayv1beta1.ListenerReasonReady),
					ObservedGeneration: gw.Generation,
					LastTransitionTime: metav1.Now(),
					Message:            "the listener is ready to route traffic",
				},
			},
		})
	}
	gw.Status.Listeners = listenersStatus

	return r.Status().Patch(ctx, gw, client.MergeFrom(previousGW))
}

func newGatewayStatus() gatewayv1beta1.GatewayStatus {
	return gatewayv1beta1.GatewayStatus{}
}
