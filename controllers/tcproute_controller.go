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
	"encoding/binary"
	"fmt"
	"time"

	"github.com/go-logr/logr"
	"k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	"sigs.k8s.io/controller-runtime/pkg/event"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/log"
	"sigs.k8s.io/controller-runtime/pkg/source"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	dataplane "github.com/kubernetes-sigs/blixt/internal/dataplane/client"
	"github.com/kubernetes-sigs/blixt/pkg/vars"
)

//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=tcproutes,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=tcproutes/status,verbs=get;update;patch
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=tcproutes/finalizers,verbs=update
//+kubebuilder:rbac:groups=core,resources=pods,verbs=get;list;watch
//+kubebuilder:rbac:groups=core,resources=pods/status,verbs=get
//+kubebuilder:rbac:groups=apps,resources=daemonsets,verbs=get;list;watch
//+kubebuilder:rbac:groups=apps,resources=daemonsets/status,verbs=get

// TCPRouteReconciler reconciles a TCPRoute object
type TCPRouteReconciler struct {
	client.Client
	Scheme *runtime.Scheme

	log                   logr.Logger
	ReconcileRequestChan  <-chan event.GenericEvent
	BackendsClientManager *dataplane.BackendsClientManager
}

// SetupWithManager sets up the controller with the Manager.
func (r *TCPRouteReconciler) SetupWithManager(mgr ctrl.Manager) error {
	r.log = log.FromContext(context.Background())

	return ctrl.NewControllerManagedBy(mgr).
		For(&gatewayv1alpha2.TCPRoute{}).
		WatchesRawSource(
			&source.Channel{Source: r.ReconcileRequestChan},
			handler.EnqueueRequestsFromMapFunc(r.mapDataPlaneDaemonsetToTCPRoutes),
		).
		Watches(
			&gatewayv1beta1.Gateway{},
			handler.EnqueueRequestsFromMapFunc(r.mapGatewayToTCPRoutes),
		).
		Complete(r)
}

// Reconcile reconciles TCPRoute object
func (r *TCPRouteReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	tcproute := new(gatewayv1alpha2.TCPRoute)
	if err := r.Get(ctx, req.NamespacedName, tcproute); err != nil {
		if errors.IsNotFound(err) {
			r.log.Info("object enqueued no longer exists, skipping")
			return ctrl.Result{}, nil
		}
		r.log.Info("Error retrieving tcp route", "Err : ", err)
		return ctrl.Result{}, err
	}

	isManaged, gateway, err := r.isTCPRouteManaged(ctx, *tcproute)
	if err != nil {
		return ctrl.Result{}, err
	}
	if !isManaged {
		// TODO: enable orphan checking https://github.com/kubernetes-sigs/blixt/issues/47
		return ctrl.Result{}, nil
	}

	if !controllerutil.ContainsFinalizer(tcproute, DataPlaneFinalizer) {
		if tcproute.DeletionTimestamp != nil {
			// if the finalizer isn't set, AND the object is being deleted then there's
			// no reason to bother with dataplane configuration for it its already
			// handled.
			return ctrl.Result{}, nil
		}
		// if the finalizer is not set, and the object is not being deleted, set the
		// finalizer before we do anything else to ensure we don't lose track of
		// dataplane configuration.
		return ctrl.Result{}, setDataPlaneFinalizer(ctx, r.Client, tcproute)
	}

	// if the TCPRoute is being deleted, remove it from the DataPlane
	// TODO: enable deletion grace period https://github.com/Kong/blixt/issues/48
	if tcproute.DeletionTimestamp != nil {
		return ctrl.Result{}, r.ensureTCPRouteDeletedInDataPlane(ctx, tcproute, gateway)
	}

	// in all other cases ensure the TCPRoute is configured in the dataplane
	if err := r.ensureTCPRouteConfiguredInDataPlane(ctx, tcproute, gateway); err != nil {
		if err.Error() == "endpoints not ready" {
			r.log.Info("endpoints not yet ready for TCPRoute, retrying", "namespace", tcproute.Namespace, "name", tcproute.Name)
			return ctrl.Result{RequeueAfter: time.Second}, nil
		}
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// isTCPRouteManaged verifies wether a provided TCPRoute is managed by this
// controller, according to it's Gateway and GatewayClass.
func (r *TCPRouteReconciler) isTCPRouteManaged(ctx context.Context, tcproute gatewayv1alpha2.TCPRoute) (bool, *gatewayv1beta1.Gateway, error) {
	var supportedGateways []gatewayv1beta1.Gateway

	//Use the retrieve objects its parent ref to look for the gateway.
	for _, parentRef := range tcproute.Spec.ParentRefs {
		//Build Gateway object to retrieve
		gw := new(gatewayv1beta1.Gateway)

		ns := tcproute.Namespace
		if parentRef.Namespace != nil {
			ns = string(*parentRef.Namespace)
		}

		//Get Gateway for TCP Route
		if err := r.Get(ctx, types.NamespacedName{Name: string(parentRef.Name), Namespace: ns}, gw); err != nil {
			if !errors.IsNotFound(err) {
				return false, nil, err
			}
			continue
		}

		//Get GatewayClass for the Gateway and match to our name of controler
		gwc := new(gatewayv1beta1.GatewayClass)
		if err := r.Get(ctx, types.NamespacedName{Name: string(gw.Spec.GatewayClassName), Namespace: ns}, gwc); err != nil {
			if !errors.IsNotFound(err) {
				return false, nil, err
			}
			continue
		}

		if gwc.Spec.ControllerName != vars.GatewayClassControllerName {
			// not managed by this implementation, check the next parent ref
			continue
		}

		//Check if referred gateway has the at least one listener with properties defined from TCPRoute parentref.
		if err := r.verifyListener(ctx, gw, parentRef); err != nil {
			// until the Gateway has a relevant listener, we can't operate on the route.
			// Updates to the relevant Gateway will re-enqueue the TCPRoute reconcilation to retry.
			r.log.Info("No matching listener found for referred gateway", "GatewayName", parentRef.Name, "GatewayPort", parentRef.Port)
			//Check next parent ref.
			continue
		}

		supportedGateways = append(supportedGateways, *gw)
	}

	if len(supportedGateways) < 1 {
		return false, nil, nil
	}

	// TODO: support multiple gateways https://github.com/Kong/blixt/issues/40
	referredGateway := &supportedGateways[0]
	r.log.Info("TCP Route appeared referring to Gateway", "Gateway ", referredGateway.Name, "GatewayClass Name", referredGateway.Spec.GatewayClassName)

	return true, referredGateway, nil
}

// verifyListener verifies that the provided gateway has at least one listener
// matching the provided ParentReference.
func (r *TCPRouteReconciler) verifyListener(_ context.Context, gw *gatewayv1beta1.Gateway, tcprouteSpec gatewayv1alpha2.ParentReference) error {
	for _, listener := range gw.Spec.Listeners {
		if (listener.Protocol == gatewayv1beta1.TCPProtocolType) && (listener.Port == gatewayv1beta1.PortNumber(*tcprouteSpec.Port)) {
			return nil
		}
	}
	return fmt.Errorf("No matching Gateway listener found for defined Parentref")
}

func (r *TCPRouteReconciler) ensureTCPRouteConfiguredInDataPlane(ctx context.Context, tcproute *gatewayv1alpha2.TCPRoute, gateway *gatewayv1beta1.Gateway) error {
	// build the dataplane configuration from the TCPRoute and its Gateway
	targets, err := dataplane.CompileTCPRouteToDataPlaneBackend(ctx, r.Client, tcproute, gateway)
	if err != nil {
		return err
	}

	if _, err = r.BackendsClientManager.Update(ctx, targets); err != nil {
		return err
	}

	r.log.Info("successful data-plane UPDATE")

	return nil
}

func (r *TCPRouteReconciler) ensureTCPRouteDeletedInDataPlane(ctx context.Context, tcproute *gatewayv1alpha2.TCPRoute, gateway *gatewayv1beta1.Gateway) error {
	// get the gateway IP and port.
	gwIP, err := dataplane.GetGatewayIP(gateway)
	if err != nil {
		return err
	}
	gatewayIP := binary.BigEndian.Uint32(gwIP.To4())
	gwPort, err := dataplane.GetGatewayPort(gateway, tcproute.Spec.ParentRefs)
	if err != nil {
		return err
	}

	vip := dataplane.Vip{
		Ip:   gatewayIP,
		Port: gwPort,
	}

	// delete the target from the dataplane
	if _, err = r.BackendsClientManager.Delete(ctx, &vip); err != nil {
		return err
	}

	r.log.Info("successful data-plane DELETE")

	oldFinalizers := tcproute.GetFinalizers()
	newFinalizers := make([]string, 0, len(oldFinalizers)-1)
	for _, finalizer := range oldFinalizers {
		if finalizer != DataPlaneFinalizer {
			newFinalizers = append(newFinalizers, finalizer)
		}
	}
	tcproute.SetFinalizers(newFinalizers)

	return r.Client.Update(ctx, tcproute)

}
