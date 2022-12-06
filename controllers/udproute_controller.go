package controllers

import (
	"context"
	"fmt"

	"k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/log"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	"github.com/kong/blixt/pkg/vars"
)

// UDPRouteReconciler reconciles a UDPRoute object
type UDPRouteReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// Verify respective gateway has at least one listener matching to what is specified in UDPRoute parentRefs.
func (r *UDPRouteReconciler) verifyListener(ctx context.Context, gw *gatewayv1beta1.Gateway, udprouteSpec gatewayv1alpha2.ParentReference) error {
	for _, listener := range gw.Spec.Listeners {
		if (listener.Protocol == gatewayv1beta1.UDPProtocolType) && (listener.Port == gatewayv1beta1.PortNumber(*udprouteSpec.Port)) {
			return nil
		}
	}
	return fmt.Errorf("No matching Gateway listener found for defined Parentref")
}

//+kubebuilder:rbac:groups=gateway.konghq.com,resources=udproutes,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=gateway.konghq.com,resources=udproutes/status,verbs=get;update;patch
//+kubebuilder:rbac:groups=gateway.konghq.com,resources=udproutes/finalizers,verbs=update

// UDProuteReconciler reconciles UDPRoute object
func (r *UDPRouteReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := log.FromContext(ctx)

	//Retrieve udp route object.
	udproute := new(gatewayv1alpha2.UDPRoute)
	if err := r.Get(ctx, req.NamespacedName, udproute); err != nil {
		if errors.IsNotFound(err) {
			log.Info("object enqueued no longer exists, skipping")
			return ctrl.Result{}, nil
		}
		log.Info("Error retrieving udp route", "Err : ", err)
		return ctrl.Result{}, err
	}

	//Use the retrieve objects its parent ref to look for the gateway.
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
			return ctrl.Result{}, err
		}

		//Get GatewayClass for the Gateway and match to our name of controler
		gwc := new(gatewayv1beta1.GatewayClass)
		if err := r.Get(ctx, types.NamespacedName{Name: string(gw.Spec.GatewayClassName), Namespace: ns}, gwc); err != nil {
			log.Info("Unable to get Gatewayclass", "Gateway", gw.Name, "Udp Route", parentRef.Name, "err", err)
			return ctrl.Result{}, err
		}

		if gwc.Spec.ControllerName != vars.GatewayClassControllerName {
			return ctrl.Result{}, nil
		}

		//Check if referred gateway has the at least one listener with properties defined from UDPRoute parentref.
		if err := r.verifyListener(ctx, gw, parentRef); err != nil {
			log.Info("No matching listener found for referred gateway", "GatewayName", parentRef.Name, "GatewayPort", parentRef.Port)
			return ctrl.Result{}, err
		}

		log.Info("UDP Route appeared referring to Gateway", "UDPRoute", parentRef.Name, "Gateway ", gw.Name, "GatewayClass, Controller Name", gwc.Spec.ControllerName, "GatewayClass Name", gwc.ObjectMeta.Name)
	}

	return ctrl.Result{}, nil
}

// SetupWithManager sets up the controller with the Manager.
func (r *UDPRouteReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		// Uncomment the following line adding a pointer to an instance of the controlled resource as an argument
		For(&gatewayv1alpha2.UDPRoute{}).
		Complete(r)
}
