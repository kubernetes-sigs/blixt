package controllers

import (
	"context"
	"fmt"
	"reflect"

	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/utils/pointer"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/log"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"
	"sigs.k8s.io/controller-runtime/pkg/source"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways/status,verbs=get;update;patch
//+kubebuilder:rbac:groups=gateway.networking.k8s.io,resources=gateways/finalizers,verbs=update

//+kubebuilder:rbac:groups=core,resources=services,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=core,resources=services/status,verbs=get

//+kubebuilder:rbac:groups=core,resources=endpoints,verbs=get;list;watch;create;update;patch;delete
//+kubebuilder:rbac:groups=core,resources=endpoints/status,verbs=get

const gatewayServiceLabel = "konghq.com/owned-by-gateway"

// GatewayReconciler reconciles a Gateway object
type GatewayReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

func (r *GatewayReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&gatewayv1beta1.Gateway{}).
		Watches(
			&source.Kind{Type: &corev1.Service{}},
			handler.EnqueueRequestsFromMapFunc(mapServiceToGateway),
		).
		Complete(r)
}

func (r *GatewayReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := log.FromContext(ctx)

	gw := new(gatewayv1beta1.Gateway)
	if err := r.Client.Get(ctx, req.NamespacedName, gw); err != nil {
		if errors.IsNotFound(err) {
			log.Info("object enqueued no longer exists, skipping")
			return ctrl.Result{}, nil
		}
		return ctrl.Result{}, err
	}

	gwc := new(gatewayv1beta1.GatewayClass)
	if err := r.Client.Get(ctx, types.NamespacedName{Name: string(gw.Spec.GatewayClassName)}, gwc); err != nil {
		if errors.IsNotFound(err) {
			return ctrl.Result{}, nil
		}
		return ctrl.Result{}, err
	}

	if gwc.Spec.ControllerName != GatewayClassControllerName {
		return ctrl.Result{}, nil
	}

	log.Info("checking for Service for Gateway")
	svc, err := r.getServiceForGateway(ctx, gw)
	if err != nil {
		return ctrl.Result{}, err
	}
	if svc == nil {
		log.Info("creating Service for Gateway")
		return ctrl.Result{}, r.createServiceForGateway(ctx, gw) // service creation will requeue gateway
	}

	log.Info("checking Service configuration")
	needsUpdate, err := r.ensureServiceConfiguration(ctx, svc, gw)
	if err != nil {
		return ctrl.Result{}, err
	}
	if needsUpdate {
		return ctrl.Result{}, r.Client.Update(ctx, svc)
	}

	log.Info("checking Service status", "namespace", svc.Namespace, "name", svc.Name)
	switch t := svc.Spec.Type; t {
	case corev1.ServiceTypeLoadBalancer:
		if svc.Spec.ClusterIP == "" || len(svc.Status.LoadBalancer.Ingress) < 1 {
			log.Info("waiting for Service to be ready")
			return ctrl.Result{Requeue: true}, nil
		}
	default:
		return ctrl.Result{}, fmt.Errorf("found unsupported Service type: %s (only LoadBalancer type is currently supported)", t)
	}

	// hack for metallb - https://github.com/metallb/metallb/issues/1640
	created, err := r.hackEnsureEndpoints(ctx, svc)
	if err != nil {
		return ctrl.Result{}, err
	}
	if created {
		return ctrl.Result{Requeue: true}, nil
	}

	log.Info("Service is ready, updating Gateway")
	return ctrl.Result{}, r.markGatewayReady(ctx, gw, svc)
}

func (r *GatewayReconciler) getServiceForGateway(ctx context.Context, gw *gatewayv1beta1.Gateway) (*corev1.Service, error) {
	svcs := new(corev1.ServiceList)
	if err := r.List(ctx, svcs, client.InNamespace(gw.Namespace), client.MatchingLabels{gatewayServiceLabel: gw.Name}); err != nil {
		return nil, err
	}

	if len(svcs.Items) > 1 {
		return nil, fmt.Errorf("more than 1 Service found for Gateway %s/%s, not currently supported", gw.Namespace, gw.Name)
	}

	for _, svc := range svcs.Items {
		return &svc, nil
	}

	return nil, nil
}

func (r *GatewayReconciler) createServiceForGateway(ctx context.Context, gw *gatewayv1beta1.Gateway) error {
	svc := corev1.Service{
		ObjectMeta: metav1.ObjectMeta{
			Namespace:    gw.Namespace,
			GenerateName: fmt.Sprintf("service-for-gateway-%s-", gw.Name),
			Labels: map[string]string{
				gatewayServiceLabel: gw.Name,
			},
		},
	}
	_, err := r.ensureServiceConfiguration(ctx, &svc, gw)
	if err != nil {
		return err
	}

	setOwnerReference(&svc, gw)

	return r.Client.Create(ctx, &svc)
}

func setOwnerReference(svc *corev1.Service, gw client.Object) {
	gvk := gw.GetObjectKind().GroupVersionKind()
	svc.ObjectMeta.OwnerReferences = []metav1.OwnerReference{{
		APIVersion: fmt.Sprintf("%s/%s", gvk.Group, gvk.Version),
		Kind:       gvk.Kind,
		Name:       gw.GetName(),
		UID:        gw.GetUID(),
		Controller: pointer.Bool(true),
	}}
}

func (r *GatewayReconciler) ensureServiceConfiguration(ctx context.Context, svc *corev1.Service, gw *gatewayv1beta1.Gateway) (bool, error) {
	ports := make([]corev1.ServicePort, 0, len(gw.Spec.Listeners))
	for _, listener := range gw.Spec.Listeners {
		switch proto := listener.Protocol; proto {
		case gatewayv1beta1.TCPProtocolType:
			ports = append(ports, corev1.ServicePort{
				Name:     string(listener.Name),
				Protocol: corev1.ProtocolTCP,
				Port:     int32(listener.Port),
			})
		case gatewayv1beta1.UDPProtocolType:
			ports = append(ports, corev1.ServicePort{
				Name:     string(listener.Name),
				Protocol: corev1.ProtocolUDP,
				Port:     int32(listener.Port),
			})
		}
	}

	updated := false
	if svc.Spec.Type != corev1.ServiceTypeLoadBalancer {
		svc.Spec.Type = corev1.ServiceTypeLoadBalancer
		updated = true
	}

	newPorts := make(map[string]portAndProtocol, len(ports))
	for _, newPort := range ports {
		newPorts[newPort.Name] = portAndProtocol{
			port:     newPort.Port,
			protocol: newPort.Protocol,
		}
	}

	oldPorts := make(map[string]portAndProtocol, len(svc.Spec.Ports))
	for _, oldPort := range svc.Spec.Ports {
		oldPorts[oldPort.Name] = portAndProtocol{
			port:     oldPort.Port,
			protocol: oldPort.Protocol,
		}
	}

	if !reflect.DeepEqual(newPorts, oldPorts) {
		svc.Spec.Ports = ports
		updated = true
	}

	return updated, nil
}

var (
	ipAddrType   = gatewayv1beta1.IPAddressType
	hostAddrType = gatewayv1beta1.HostnameAddressType
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

	return r.Status().Patch(ctx, gw, client.MergeFrom(previousGW))
}

// hackEnsureEndpoints is a temporary hack around how metallb'd L2 mode works, re: https://github.com/metallb/metallb/issues/1640
func (r *GatewayReconciler) hackEnsureEndpoints(ctx context.Context, svc *corev1.Service) (bool, error) {
	nsn := types.NamespacedName{Namespace: svc.Namespace, Name: svc.Name}
	lbaddr := ""
	for _, addr := range svc.Status.LoadBalancer.Ingress {
		if addr.IP != "" {
			lbaddr = addr.IP
			break
		}
		if addr.Hostname != "" {
			lbaddr = addr.Hostname
			break
		}
	}

	endpoints := new(corev1.Endpoints)
	err := r.Client.Get(ctx, nsn, endpoints)
	if err != nil {
		if errors.IsNotFound(err) {
			eports := make([]corev1.EndpointPort, 0, len(svc.Spec.Ports))
			for _, svcPort := range svc.Spec.Ports {
				eports = append(eports, corev1.EndpointPort{
					Port:     svcPort.Port,
					Protocol: svcPort.Protocol,
				})
			}

			endpoints = &corev1.Endpoints{
				ObjectMeta: metav1.ObjectMeta{
					Namespace: svc.Namespace,
					Name:      svc.Name,
				},
				Subsets: []corev1.EndpointSubset{{
					Addresses: []corev1.EndpointAddress{{IP: lbaddr}},
					Ports:     eports,
				}},
			}

			return true, r.Client.Create(ctx, endpoints)
		}
		return false, err
	}

	return false, nil
}

func mapServiceToGateway(obj client.Object) (reqs []reconcile.Request) {
	svc, ok := obj.(*corev1.Service)
	if !ok {
		return
	}

	for _, ownerRef := range svc.OwnerReferences {
		if ownerRef.APIVersion == fmt.Sprintf("%s/%s", gatewayv1beta1.GroupName, gatewayv1beta1.GroupVersion.Version) {
			reqs = append(reqs, reconcile.Request{
				NamespacedName: types.NamespacedName{
					Namespace: svc.Namespace,
					Name:      ownerRef.Name,
				},
			})
		}
	}

	return
}

type portAndProtocol struct {
	port     int32
	protocol corev1.Protocol
}
