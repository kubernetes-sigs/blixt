package controllers

import (
	"context"
	"fmt"
	"time"

	"github.com/go-logr/logr"
	appsv1 "k8s.io/api/apps/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/log"
	"sigs.k8s.io/controller-runtime/pkg/source"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	dataplane "github.com/kong/blixt/internal/dataplane/client"
	"github.com/kong/blixt/pkg/vars"
)

//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=udproutes,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=udproutes/status,verbs=get;update;patch
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=udproutes/finalizers,verbs=update
//+kubebuilder:rbac:groups=core,resources=pods,verbs=get;list;watch
//+kubebuilder:rbac:groups=core,resources=pods/status,verbs=get
//+kubebuilder:rbac:groups=apps,resources=daemonsets,verbs=get;list;watch
//+kubebuilder:rbac:groups=apps,resources=daemonsets/status,verbs=get

// UDPRouteReconciler reconciles a UDPRoute object
type UDPRouteReconciler struct {
	client.Client
	Scheme *runtime.Scheme

	log logr.Logger
}

// SetupWithManager sets up the controller with the Manager.
func (r *UDPRouteReconciler) SetupWithManager(mgr ctrl.Manager) error {
	r.log = log.FromContext(context.Background())

	return ctrl.NewControllerManagedBy(mgr).
		For(&gatewayv1alpha2.UDPRoute{}).
		Watches(
			&source.Kind{Type: &appsv1.DaemonSet{}},
			handler.EnqueueRequestsFromMapFunc(r.mapDataPlaneDaemonsetToUDPRoutes),
		).
		Complete(r)
}

// UDProuteReconciler reconciles UDPRoute object
func (r *UDPRouteReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	udproute := new(gatewayv1alpha2.UDPRoute)
	if err := r.Get(ctx, req.NamespacedName, udproute); err != nil {
		if errors.IsNotFound(err) {
			r.log.Info("object enqueued no longer exists, skipping")
			return ctrl.Result{}, nil
		}
		r.log.Info("Error retrieving udp route", "Err : ", err)
		return ctrl.Result{}, err
	}

	isManaged, gateway, err := r.isUDPRouteManaged(ctx, *udproute)
	if !isManaged {
		// TODO: enable orphan checking https://github.com/Kong/blixt/issues/47
		return ctrl.Result{}, err
	}

	if !controllerutil.ContainsFinalizer(udproute, DataPlaneFinalizer) {
		if udproute.DeletionTimestamp != nil {
			// if the finalizer isn't set, AND the object is being deleted then there's
			// no reason to bother with dataplane configuration for it its already
			// handled.
			return ctrl.Result{}, nil
		}
		// if the finalizer is not set, and the object is not being deleted, set the
		// finalizer before we do anything else to ensure we don't lose track of
		// dataplane configuration.
		return ctrl.Result{}, setDataPlaneFinalizer(ctx, r.Client, udproute)
	}

	// if the UDPRoute is being deleted, remove it from the DataPlane
	// TODO: enable deletion grace period https://github.com/Kong/blixt/issues/48
	if udproute.DeletionTimestamp != nil {
		return ctrl.Result{}, r.ensureUDPRouteDeletedInDataPlane(ctx, udproute, gateway)
	}

	// in all other cases ensure the UDPRoute is configured in the dataplane
	if err := r.ensureUDPRouteConfiguredInDataPlane(ctx, udproute, gateway); err != nil {
		if err.Error() == "endpoints not ready" {
			r.log.Info("endpoints not yet ready for UDPRoute, retrying", "namespace", udproute.Namespace, "name", udproute.Name)
			return ctrl.Result{RequeueAfter: time.Second}, nil
		}
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// isUDPRouteManaged verifies wether a provided UDPRoute is managed by this
// controller, according to it's Gateway and GatewayClass.
func (r *UDPRouteReconciler) isUDPRouteManaged(ctx context.Context, udproute gatewayv1alpha2.UDPRoute) (bool, *gatewayv1beta1.Gateway, error) {
	log := log.FromContext(ctx)

	var supportedGateways []gatewayv1beta1.Gateway

	//Use the retrieve objects its parent ref to look for the gateway.
	var parentRefErr error
	for _, parentRef := range udproute.Spec.ParentRefs {

		//Build Gateway object to retrieve
		gw := new(gatewayv1beta1.Gateway)

		ns := udproute.Namespace
		if parentRef.Namespace != nil {
			ns = string(*parentRef.Namespace)
		}

		//Get Gateway for UDP Route
		if err := r.Get(ctx, types.NamespacedName{Name: string(parentRef.Name), Namespace: ns}, gw); err != nil {
			log.Info("Unable to get parent ref gateway for", "udpRoute", parentRef.Name, "Namespace", ns, "err", err)
			parentRefErr = err
			//Check next parent ref.
			continue
		}

		//Get GatewayClass for the Gateway and match to our name of controler
		gwc := new(gatewayv1beta1.GatewayClass)
		if err := r.Get(ctx, types.NamespacedName{Name: string(gw.Spec.GatewayClassName), Namespace: ns}, gwc); err != nil {
			log.Info("Unable to get Gatewayclass", "Gateway", gw.Name, "Udp Route", parentRef.Name, "err", err)
			parentRefErr = err
			//Check next parent ref.
			continue
		}

		if gwc.Spec.ControllerName != vars.GatewayClassControllerName {
			//Check next parent ref.
			continue
		}

		//Check if referred gateway has the at least one listener with properties defined from UDPRoute parentref.
		if err := r.verifyListener(ctx, gw, parentRef); err != nil {
			log.Info("No matching listener found for referred gateway", "GatewayName", parentRef.Name, "GatewayPort", parentRef.Port)
			//Check next parent ref.
			continue
		}

		supportedGateways = append(supportedGateways, *gw)
	}

	//only used for logging
	if len(supportedGateways) < 1 {
		return false, nil, parentRefErr
	}

	refferedGateway := &supportedGateways[0]
	log.Info("UDP Route appeared referring to Gateway", "Gateway ", refferedGateway.Name, "GatewayClass Name", refferedGateway.Spec.GatewayClassName)

	return true, refferedGateway, nil
}

// verifyListener verifies that the provided gateway has at least one listener
// matching the provided ParentReference.
func (r *UDPRouteReconciler) verifyListener(ctx context.Context, gw *gatewayv1beta1.Gateway, udprouteSpec gatewayv1alpha2.ParentReference) error {
	for _, listener := range gw.Spec.Listeners {
		if (listener.Protocol == gatewayv1beta1.UDPProtocolType) && (listener.Port == gatewayv1beta1.PortNumber(*udprouteSpec.Port)) {
			return nil
		}
	}
	return fmt.Errorf("No matching Gateway listener found for defined Parentref")
}

func (r *UDPRouteReconciler) ensureUDPRouteConfiguredInDataPlane(ctx context.Context, udproute *gatewayv1alpha2.UDPRoute, gateway *gatewayv1beta1.Gateway) error {
	// build the dataplane configuration from the UDPRoute and its Gateway
	targets, err := dataplane.CompileUDPRouteToDataPlaneBackend(ctx, r.Client, udproute, gateway)
	if err != nil {
		return err
	}

	// TODO: add multiple endpoint support https://github.com/Kong/blixt/issues/46
	dataplaneClient, err := dataplane.NewDataPlaneClient(context.Background(), r.Client)
	if err != nil {
		return err
	}

	confirmation, err := dataplaneClient.Update(context.Background(), targets)
	if err != nil {
		return err
	}

	r.log.Info(fmt.Sprintf("successful data-plane UPDATE, confirmation: %s", confirmation.String()))

	return nil
}

func (r *UDPRouteReconciler) ensureUDPRouteDeletedInDataPlane(ctx context.Context, udproute *gatewayv1alpha2.UDPRoute, gateway *gatewayv1beta1.Gateway) error {
	// build the dataplane configuration from the UDPRoute and its Gateway
	targets, err := dataplane.CompileUDPRouteToDataPlaneBackend(ctx, r.Client, udproute, gateway)
	if err != nil {
		return err
	}

	// TODO: add multiple endpoint support https://github.com/Kong/blixt/issues/46
	dataplaneClient, err := dataplane.NewDataPlaneClient(context.Background(), r.Client)
	if err != nil {
		return err
	}

	// delete the target from the dataplane
	confirmation, err := dataplaneClient.Delete(context.Background(), targets.Vip)
	if err != nil {
		return err
	}

	r.log.Info(fmt.Sprintf("successful data-plane DELETE, confirmation: %s", confirmation.String()))

	oldFinalizers := udproute.GetFinalizers()
	newFinalizers := make([]string, 0, len(oldFinalizers)-1)
	for _, finalizer := range oldFinalizers {
		if finalizer != DataPlaneFinalizer {
			newFinalizers = append(newFinalizers, finalizer)
		}
	}
	udproute.SetFinalizers(newFinalizers)

	return r.Client.Update(ctx, udproute)
}
