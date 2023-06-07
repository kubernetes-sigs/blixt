package controllers

import (
	"context"
	"fmt"
	"reflect"

	"github.com/kong/blixt/pkg/vars"
	appsv1 "k8s.io/api/apps/v1"
	"k8s.io/apimachinery/pkg/types"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

// mapDataPlaneDaemonsetToUDPRoutes is a mapping function to map dataplane
// DaemonSet updates to UDPRoute reconcilations. This enables changes to the
// DaemonSet such as adding new Pods for a new Node to result in new dataplane
// instances getting fully configured.
func (r *UDPRouteReconciler) mapDataPlaneDaemonsetToUDPRoutes(ctx context.Context, obj client.Object) (reqs []reconcile.Request) {
	daemonset, ok := obj.(*appsv1.DaemonSet)
	if !ok {
		return
	}

	// determine if this is a blixt daemonset
	matchLabels := daemonset.Spec.Selector.MatchLabels
	app, ok := matchLabels["app"]
	if !ok || app != vars.DefaultDataPlaneAppLabel {
		return
	}

	// verify that it's the dataplane daemonset
	component, ok := matchLabels["component"]
	if !ok || component != vars.DefaultDataPlaneComponentLabel {
		return
	}

	udproutes := &gatewayv1alpha2.UDPRouteList{}
	if err := r.Client.List(ctx, udproutes); err != nil {
		// TODO: https://github.com/kubernetes-sigs/controller-runtime/issues/1996
		r.log.Error(err, "could not enqueue UDPRoutes for DaemonSet update")
		return
	}

	for _, udproute := range udproutes.Items {
		reqs = append(reqs, reconcile.Request{
			NamespacedName: types.NamespacedName{
				Namespace: udproute.Namespace,
				Name:      udproute.Name,
			},
		})
	}

	return
}

// mapGatewayToUDPRoutes enqueues reconcilation for all UDPRoutes whenever
// an event occurs on a relevant Gateway.
func (r *UDPRouteReconciler) mapGatewayToUDPRoutes(ctx context.Context, obj client.Object) (reqs []reconcile.Request) {
	gateway, ok := obj.(*gatewayv1beta1.Gateway)
	if !ok {
		r.log.Error(fmt.Errorf("invalid type in map func"), "failed to map gateways to udproutes", "expected", "*gatewayv1beta1.Gateway", "received", reflect.TypeOf(obj))
		return
	}

	udproutes := new(gatewayv1alpha2.UDPRouteList)
	if err := r.Client.List(context.Background(), udproutes); err != nil {
		// TODO: https://github.com/kubernetes-sigs/controller-runtime/issues/1996
		r.log.Error(err, "could not enqueue UDPRoutes for Gateway update")
		return
	}

	for _, udproute := range udproutes.Items {
		for _, parentRef := range udproute.Spec.ParentRefs {
			namespace := udproute.Namespace
			if parentRef.Namespace != nil {
				namespace = string(*parentRef.Namespace)
			}
			if parentRef.Name == gatewayv1alpha2.ObjectName(gateway.Name) && namespace == gateway.Namespace {
				reqs = append(reqs, reconcile.Request{NamespacedName: types.NamespacedName{
					Namespace: udproute.Namespace,
					Name:      udproute.Name,
				}})
			}
		}
	}

	return
}
