package controllers

import (
	"context"
	"fmt"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/builder"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/event"
	"sigs.k8s.io/controller-runtime/pkg/log"
	"sigs.k8s.io/controller-runtime/pkg/predicate"

	dataplane "github.com/kong/blixt/internal/dataplane/client"
	"github.com/kong/blixt/pkg/vars"
)

//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways/status,verbs=get;update;patch
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways/finalizers,verbs=update

//+kubebuilder:rbac:groups=core,resources=services,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=core,resources=services/status,verbs=get

//+kubebuilder:rbac:groups=core,resources=endpoints,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=core,resources=endpoints/status,verbs=get

// DataplaneReconciler reconciles the dataplane pods.
type DataplaneReconciler struct {
	client.Client
	scheme *runtime.Scheme

	backendsClientManager *dataplane.BackendsClientManager

	updates chan event.GenericEvent
}

func NewDataplaneReconciler(client client.Client, schema *runtime.Scheme, manager *dataplane.BackendsClientManager) *DataplaneReconciler {
	return &DataplaneReconciler{
		Client:                client,
		scheme:                schema,
		backendsClientManager: manager,
		updates:               make(chan event.GenericEvent, 1),
	}
}

var (
	podOwnerKey = ".metadata.controller"
	apiGVStr    = appsv1.SchemeGroupVersion.String()
)

// SetupWithManager loads the controller into the provided controller manager.
func (r *DataplaneReconciler) SetupWithManager(mgr ctrl.Manager) error {

	// In order to allow our reconciler to quickly look up Pods by their owner, we’ll
	// need an index. We declare an index key that we can later use with the client
	// as a pseudo-field name, and then describe how to extract the indexed value from
	// the Pod object. The indexer will automatically take care of namespaces for us,
	// so we just have to extract the owner name if the Pod has a DaemonSet owner.
	if err := mgr.GetFieldIndexer().IndexField(context.Background(), &corev1.Pod{}, podOwnerKey, func(rawObj client.Object) []string {
		// grab the pod object, extract the owner...
		pod := rawObj.(*corev1.Pod)
		owner := metav1.GetControllerOf(pod)
		if owner == nil {
			return nil
		}
		// ...make sure it's a DaemonSet...
		if owner.APIVersion != apiGVStr || owner.Kind != "DaemonSet" {
			return nil
		}

		// ...and if so, return it
		return []string{owner.Name}
	}); err != nil {
		return err
	}

	return ctrl.NewControllerManagedBy(mgr).
		For(&appsv1.DaemonSet{},
			builder.WithPredicates(predicate.NewPredicateFuncs(r.daemonsetHasMatchingAnnotations)),
		).
		Complete(r)
}

func (r *DataplaneReconciler) daemonsetHasMatchingAnnotations(obj client.Object) bool {
	log := log.FromContext(context.Background())

	daemonset, ok := obj.(*appsv1.DaemonSet)
	if !ok {
		log.Error(fmt.Errorf("received unexpected type in daemonset watch predicates: %T", obj), "THIS SHOULD NEVER HAPPEN!")
		return false
	}

	// determine if this is a blixt daemonset
	matchLabels := daemonset.Spec.Selector.MatchLabels
	app, ok := matchLabels["app"]
	if !ok || app != vars.DefaultDataPlaneAppLabel {
		return false
	}

	// verify that it's the dataplane daemonset
	component, ok := matchLabels["component"]
	if !ok || component != vars.DefaultDataPlaneComponentLabel {
		return false
	}

	return true
}

// Reconcile provisions (and de-provisions) resources relevant to this controller.
func (r *DataplaneReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	logger := log.FromContext(ctx)

	ds := new(appsv1.DaemonSet)
	if err := r.Client.Get(ctx, req.NamespacedName, ds); err != nil {
		if errors.IsNotFound(err) {
			logger.Info("DataplaneReconciler", "reconcile status", "object enqueued no longer exists, skipping")
			return ctrl.Result{}, nil
		}
		return ctrl.Result{}, err
	}

	var childPods corev1.PodList
	if err := r.List(ctx, &childPods, client.InNamespace(req.Namespace), client.MatchingFields{podOwnerKey: req.Name}); err != nil {
		logger.Error(err, "DataplaneReconciler", "reconcile status", "unable to list child pods")
		return ctrl.Result{}, err
	}

	readyPodByNN := make(map[types.NamespacedName]corev1.Pod)
	for _, pod := range childPods.Items {
		for _, container := range pod.Status.ContainerStatuses {
			if container.Name == vars.DefaultDataPlaneComponentLabel && container.Ready {
				key := types.NamespacedName{Namespace: pod.Namespace, Name: pod.Name}
				readyPodByNN[key] = pod
			}
		}
	}

	logger.Info("DataplaneReconciler", "reconcile status", "setting updated backends client list", "num ready pods", len(readyPodByNN))
	updated, err := r.backendsClientManager.SetClientsList(ctx, readyPodByNN)
	if updated {
		logger.Info("DataplaneReconciler", "reconcile status", "backends client list updated, sending generic event")
		select {
		case r.updates <- event.GenericEvent{Object: ds}:
			logger.Info("DataplaneReconciler", "reconcile status", "generic event sent")
		default:
			logger.Info("DataplaneReconciler", "reconcile status", "generic event skipped - channel is full")
		}
	}
	if err != nil {
		logger.Error(err, "DataplaneReconciler", "reconcile status", "partial failure for backends client list update")
		return ctrl.Result{Requeue: true}, err
	}

	logger.Info("DataplaneReconciler", "reconcile status", "done")
	return ctrl.Result{}, nil
}

func (r *DataplaneReconciler) GetUpdates() <-chan event.GenericEvent {
	return r.updates
}
