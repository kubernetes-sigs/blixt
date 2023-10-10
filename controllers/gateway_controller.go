package controllers

import (
	"context"
	"fmt"
	"reflect"
	"strings"
	"time"

	"github.com/go-logr/logr"
	"github.com/kong/blixt/pkg/vars"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/builder"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/log"
	"sigs.k8s.io/controller-runtime/pkg/predicate"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways/status,verbs=get;update;patch
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways/finalizers,verbs=update

//+kubebuilder:rbac:groups=core,resources=services,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=core,resources=services/status,verbs=get

//+kubebuilder:rbac:groups=core,resources=endpoints,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=core,resources=endpoints/status,verbs=get

//+kubebuilder:rbac:groups=core,resources=events,verbs=get;list;watch

const gatewayServiceLabel = "konghq.com/owned-by-gateway"

// GatewayReconciler reconciles a Gateway object
type GatewayReconciler struct {
	client.Client
	Scheme *runtime.Scheme
	Log    logr.Logger
}

// SetupWithManager loads the controller into the provided controller manager.
func (r *GatewayReconciler) SetupWithManager(mgr ctrl.Manager) error {
	r.Log = log.FromContext(context.Background())

	return ctrl.NewControllerManagedBy(mgr).
		For(&gatewayv1beta1.Gateway{},
			builder.WithPredicates(predicate.NewPredicateFuncs(r.gatewayHasMatchingGatewayClass)),
		).
		Watches(
			&corev1.Service{},
			handler.EnqueueRequestsFromMapFunc(mapServiceToGateway),
		).
		Watches(
			&gatewayv1beta1.GatewayClass{},
			handler.EnqueueRequestsFromMapFunc(r.mapGatewayClassToGateway),
		).
		Complete(r)
}

func (r *GatewayReconciler) gatewayHasMatchingGatewayClass(obj client.Object) bool {
	gateway, ok := obj.(*gatewayv1beta1.Gateway)
	if !ok {
		r.Log.Error(fmt.Errorf("unexpected object type in gateway watch predicates"), "expected", "*gatewayv1beta1.Gateway", "found", reflect.TypeOf(obj))
		return false
	}

	gatewayClass := &gatewayv1beta1.GatewayClass{}
	if err := r.Client.Get(context.Background(), client.ObjectKey{Name: string(gateway.Spec.GatewayClassName)}, gatewayClass); err != nil {
		if errors.IsNotFound(err) {
			return false
		}
		r.Log.Error(err, "couldn't retrieve gatewayclass for unknown reason, enqueing gateway anyway to avoid miss", "gatewayclass", gateway.Spec.GatewayClassName)
		return true
	}

	return gatewayClass.Spec.ControllerName == vars.GatewayClassControllerName
}

// Reconcile provisions (and de-provisions) resources relevant to this controller.
// TODO: this whole thing needs a rewrite
func (r *GatewayReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := log.FromContext(ctx)

	gateway := new(gatewayv1beta1.Gateway)
	if err := r.Client.Get(ctx, req.NamespacedName, gateway); err != nil {
		if errors.IsNotFound(err) {
			log.Info("object enqueued no longer exists, skipping")
			return ctrl.Result{}, nil
		}
		return ctrl.Result{}, err
	}

	gatewayClass := new(gatewayv1beta1.GatewayClass)
	if err := r.Client.Get(ctx, types.NamespacedName{Name: string(gateway.Spec.GatewayClassName)}, gatewayClass); err != nil {
		if errors.IsNotFound(err) {
			return ctrl.Result{}, nil
		}
		return ctrl.Result{}, err
	}

	if gatewayClass.Spec.ControllerName != vars.GatewayClassControllerName {
		return ctrl.Result{}, nil
	}

	log.Info("found a supported Gateway, determining whether the gateway has been accepted")
	oldGateway := gateway.DeepCopy()
	if !isGatewayAccepted(gateway) {
		log.Info("gateway not yet accepted")
		setGatewayListenerStatus(gateway)
		setGatewayStatus(gateway)
		updateConditionGeneration(gateway)
		return ctrl.Result{}, r.Status().Patch(ctx, gateway, client.MergeFrom(oldGateway))
	}

	log.Info("checking for Service for Gateway")
	svc, err := r.getServiceForGateway(ctx, gateway)
	if err != nil {
		return ctrl.Result{}, err
	}
	if svc == nil {
		log.Info("creating Service for Gateway")
		return ctrl.Result{}, r.createServiceForGateway(ctx, gateway) // service creation will requeue gateway
	}

	log.Info("checking Service configuration")
	needsUpdate, err := r.ensureServiceConfiguration(ctx, svc, gateway)
	if err != nil {
		return ctrl.Result{}, err
	}
	if needsUpdate {
		return ctrl.Result{}, r.Client.Update(ctx, svc)
	}

	log.Info("checking Service status", "namespace", svc.Namespace, "name", svc.Name)
	switch t := svc.Spec.Type; t {
	case corev1.ServiceTypeLoadBalancer:
		if err := r.svcIsHealthy(ctx, svc); err != nil {
			// TODO: only handles metallb right now https://github.com/Kong/blixt/issues/96
			if strings.Contains(err.Error(), "Failed to allocate IP") {
				r.Log.Info("failed to allocate IP for Gateway", gateway.Namespace, gateway.Name)
				setCond(gateway, metav1.Condition{
					Type:               string(gatewayv1beta1.GatewayConditionProgrammed),
					ObservedGeneration: gateway.Generation,
					Status:             metav1.ConditionFalse,
					LastTransitionTime: metav1.Now(),
					Reason:             string(gatewayv1beta1.GatewayReasonAddressNotUsable),
					Message:            err.Error(),
				})
				updateConditionGeneration(gateway)
				return ctrl.Result{Requeue: true}, r.Status().Patch(ctx, gateway, client.MergeFrom(oldGateway))
			}
			return ctrl.Result{}, err
		}

		if svc.Spec.ClusterIP == "" || len(svc.Status.LoadBalancer.Ingress) < 1 {
			log.Info("waiting for Service to be ready")
			return ctrl.Result{RequeueAfter: time.Second}, nil
		}
	default:
		return ctrl.Result{}, fmt.Errorf("found unsupported Service type: %s (only LoadBalancer type is currently supported)", t)
	}

	// hack for metallb - https://github.com/metallb/metallb/issues/1640
	// no need to enforce the gateway status here, as this endpoint is not reconciled by the controller
	// and no reconciliation loop is triggered upon its change or deletion.
	created, err := r.hackEnsureEndpoints(ctx, svc)
	if err != nil {
		return ctrl.Result{}, err
	}
	if created {
		return ctrl.Result{Requeue: true}, nil
	}

	log.Info("Service is ready, setting Gateway as programmed")
	setGatewayStatusAddresses(gateway, svc)
	setGatewayListenerConditionsAndProgrammed(gateway)
	updateConditionGeneration(gateway)
	return ctrl.Result{}, r.Status().Patch(ctx, gateway, client.MergeFrom(oldGateway))
}
