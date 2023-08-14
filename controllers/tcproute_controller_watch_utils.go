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

	"github.com/kubernetes-sigs/blixt/pkg/vars"
	appsv1 "k8s.io/api/apps/v1"
	"k8s.io/apimachinery/pkg/types"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

// mapDataPlaneDaemonsetToTCPRoutes is a mapping function to map dataplane
// DaemonSet updates to TCPRoute reconcilations. This enables changes to the
// DaemonSet such as adding new Pods for a new Node to result in new dataplane
// instances getting fully configured.
func (r *TCPRouteReconciler) mapDataPlaneDaemonsetToTCPRoutes(ctx context.Context, obj client.Object) (reqs []reconcile.Request) {
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

	tcproutes := &gatewayv1alpha2.TCPRouteList{}
	if err := r.Client.List(ctx, tcproutes); err != nil {
		// TODO: https://github.com/kubernetes-sigs/controller-runtime/issues/1996
		r.log.Error(err, "could not enqueue TCPRoutes for DaemonSet update")
		return
	}

	for _, tcproute := range tcproutes.Items {
		reqs = append(reqs, reconcile.Request{
			NamespacedName: types.NamespacedName{
				Namespace: tcproute.Namespace,
				Name:      tcproute.Name,
			},
		})
	}

	return
}

// mapGatewayToTCPRoutes enqueues reconcilation for all TCPRoutes whenever
// an event occurs on a relevant Gateway.
func (r *TCPRouteReconciler) mapGatewayToTCPRoutes(_ context.Context, obj client.Object) (reqs []reconcile.Request) {
	gateway, ok := obj.(*gatewayv1beta1.Gateway)
	if !ok {
		r.log.Error(fmt.Errorf("invalid type in map func"), "failed to map gateways to tcproutes", "expected", "*gatewayv1beta1.Gateway", "received", reflect.TypeOf(obj))
		return
	}

	tcproutes := new(gatewayv1alpha2.TCPRouteList)
	if err := r.Client.List(context.Background(), tcproutes); err != nil {
		// TODO: https://github.com/kubernetes-sigs/controller-runtime/issues/1996
		r.log.Error(err, "could not enqueue TCPRoutes for Gateway update")
		return
	}

	for _, tcproute := range tcproutes.Items {
		for _, parentRef := range tcproute.Spec.ParentRefs {
			namespace := tcproute.Namespace
			if parentRef.Namespace != nil {
				namespace = string(*parentRef.Namespace)
			}
			if parentRef.Name == gatewayv1alpha2.ObjectName(gateway.Name) && namespace == gateway.Namespace {
				reqs = append(reqs, reconcile.Request{NamespacedName: types.NamespacedName{
					Namespace: tcproute.Namespace,
					Name:      tcproute.Name,
				}})
			}
		}
	}

	return
}
