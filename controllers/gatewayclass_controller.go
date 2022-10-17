package controllers

import (
	"context"

	"k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/log"
	"sigs.k8s.io/controller-runtime/pkg/predicate"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gatewayclasses,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gatewayclasses/status,verbs=get;update;patch
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gatewayclasses/finalizers,verbs=update

const GatewayClassControllerName = "konghq.com/blixt"

// GatewayClassReconciler reconciles a GatewayClass object
type GatewayClassReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

func (r *GatewayClassReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&gatewayv1beta1.GatewayClass{}).
		WithEventFilter(predicate.NewPredicateFuncs(func(obj client.Object) bool {
			gwc, ok := obj.(*gatewayv1beta1.GatewayClass)
			if !ok {
				return false
			}
			return gwc.Spec.ControllerName == GatewayClassControllerName // filter out unmanaged GWCs
		})).
		Complete(r)
}

func (r *GatewayClassReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := log.FromContext(ctx)

	gwc := new(gatewayv1beta1.GatewayClass)
	if err := r.Client.Get(ctx, req.NamespacedName, gwc); err != nil {
		if errors.IsNotFound(err) {
			log.Info("object enqueued no longer exists, skipping")
			return ctrl.Result{}, nil
		}
		return ctrl.Result{}, err
	}

	if gwc.Spec.ControllerName != GatewayClassControllerName {
		return ctrl.Result{}, nil
	}

	if !r.isAccepted(gwc) {
		log.Info("marking GatwayClass as accepted", "name", gwc.Name)
		return ctrl.Result{}, r.accept(ctx, gwc)
	}

	return ctrl.Result{}, nil

}

func (r *GatewayClassReconciler) isAccepted(gwc *gatewayv1beta1.GatewayClass) bool {
	for _, cond := range gwc.Status.Conditions {
		if cond.Type == string(gatewayv1beta1.GatewayClassConditionStatusAccepted) {
			if cond.Status == metav1.ConditionTrue {
				return true
			}
		}
	}

	return false
}

func (r *GatewayClassReconciler) accept(ctx context.Context, gwc *gatewayv1beta1.GatewayClass) error {
	previousGWC := gwc.DeepCopy()
	acceptedCond := metav1.Condition{
		Type:               string(gatewayv1beta1.GatewayClassConditionStatusAccepted),
		Status:             metav1.ConditionTrue,
		ObservedGeneration: gwc.Generation,
		LastTransitionTime: metav1.Now(),
		Reason:             string(gatewayv1beta1.GatewayClassReasonAccepted),
		Message:            "the gatewayclass has been accepted by the operator",
	}
	setCondition(acceptedCond, gwc)
	return r.Status().Patch(ctx, gwc, client.MergeFrom(previousGWC))
}

func setCondition(condition metav1.Condition, gwc *gatewayv1beta1.GatewayClass) {
	newConds := make([]metav1.Condition, 0, len(gwc.Status.Conditions))

	for i := 0; i < len(gwc.Status.Conditions); i++ {
		if gwc.Status.Conditions[i].Type != condition.Type {
			newConds = append(newConds, gwc.Status.Conditions[i])
		}
	}

	newConds = append(newConds, condition)
	gwc.Status.Conditions = newConds
}
