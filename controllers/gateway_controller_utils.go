package controllers

import (
	"context"
	"fmt"
	"reflect"

	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/utils/pointer"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

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

func (r *GatewayReconciler) mapGatewayClassToGateway(obj client.Object) (recs []reconcile.Request) {
	gatewayClass, ok := obj.(*gatewayv1beta1.GatewayClass)
	if !ok {
		r.Log.Error(fmt.Errorf("unexpected object type in gateway watch predicates"), "expected", "*gatewayv1beta1.GatewayClass", "found", reflect.TypeOf(obj))
		return
	}

	gateways := &gatewayv1beta1.GatewayList{}
	if err := r.Client.List(context.Background(), gateways); err != nil {
		// TODO: https://github.com/kubernetes-sigs/controller-runtime/issues/1996
		r.Log.Error(err, "could not map gatewayclass event to gateways")
		return
	}

	for _, gateway := range gateways.Items {
		if gateway.Spec.GatewayClassName == gatewayv1beta1.ObjectName(gatewayClass.Name) {
			recs = append(recs, reconcile.Request{NamespacedName: types.NamespacedName{
				Namespace: gateway.Namespace,
				Name:      gateway.Name,
			}})
		}
	}

	return
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
