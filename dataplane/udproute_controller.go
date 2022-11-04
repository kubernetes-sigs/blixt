package main

import (
	"context"
	"log"
	"time"

	"github.com/cilium/ebpf"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/tools/cache"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	"sigs.k8s.io/gateway-api/apis/v1beta1"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
	gwinf "sigs.k8s.io/gateway-api/pkg/client/informers/externalversions"
)

func startUDPRouteController(ctx context.Context) error {
	factory := gwinf.NewSharedInformerFactory(gwc, 5*time.Second)
	udpInformer := factory.Gateway().V1alpha2().UDPRoutes()
	udpInformer.Informer().AddEventHandler(cache.ResourceEventHandlerFuncs{
		AddFunc: func(obj interface{}) {
			udproute := obj.(*gatewayv1alpha2.UDPRoute)
			gateway, listener, isManaged := isUDPRouteManaged(udproute)
			if !isManaged {
				log.Printf("ignoring unmanaged UDPRoute %s", nsn(udproute))
				return
			}
			addUDPRouteToLB(udproute, gateway, listener)
		},
		UpdateFunc: func(old, obj interface{}) {
			udproute := obj.(*gatewayv1alpha2.UDPRoute)
			oldRoute := old.(*gatewayv1alpha2.UDPRoute)

			gateway, listener, newRouteManaged := isUDPRouteManaged(udproute)
			oldGW, oldLst, oldRouteManaged := isUDPRouteManaged(oldRoute)

			if newRouteManaged {
				addUDPRouteToLB(udproute, gateway, listener)
			} else if oldRouteManaged {
				deleteUDPRouteFromLB(udproute, oldGW, oldLst)
			}
		},
		DeleteFunc: func(obj interface{}) {
			udproute := obj.(*gatewayv1alpha2.UDPRoute)
			gateway, listener, isManaged := isUDPRouteManaged(udproute)
			if !isManaged {
				return
			}
			deleteUDPRouteFromLB(udproute, gateway, listener)
		},
	})

	factory.Start(ctx.Done())

	log.Printf("UDPRoute controller started")

	return nil
}

func isUDPRouteManaged(udproute *gatewayv1alpha2.UDPRoute) (*gatewayv1beta1.Gateway, *gatewayv1beta1.Listener, bool) {
	for _, ref := range udproute.Spec.ParentRefs {
		if ref.Port == nil {
			log.Printf("no port ref in UDPRoute %s, required currently", nsn(udproute))
			continue
		}

		namespace := udproute.Namespace
		if ref.Namespace != nil {
			namespace = string(*ref.Namespace)
		}

		gw, err := gwc.GatewayV1beta1().Gateways(namespace).Get(context.TODO(), string(ref.Name), metav1.GetOptions{})
		if err == nil {
			log.Printf("found Gateway %s/%s for UDPRoute %s", gw.Namespace, gw.Name, nsn(udproute))
			gwclass, err := gwc.GatewayV1beta1().GatewayClasses().Get(context.TODO(), string(gw.Spec.GatewayClassName), metav1.GetOptions{})
			if err == nil && gwclass.Spec.ControllerName == "konghq.com/blixt" {
				log.Printf("found GatewayClass %s for UDPRoute %s", gwclass.Name, nsn(udproute))
				for _, listener := range gw.Spec.Listeners {
					if listener.Port == v1beta1.PortNumber(*ref.Port) {
						return gw, &listener, true
					}
				}
			}
		}
	}

	return nil, nil, false
}

func isUDPRouteReady(udproute *gatewayv1alpha2.UDPRoute) (*corev1.Endpoints, bool) {
	if len(udproute.Spec.Rules) < 1 {
		log.Printf("no rules for UDPRoute %s", nsn(udproute))
		return nil, false
	}

	if len(udproute.Spec.Rules[0].BackendRefs) < 1 {
		log.Printf("no backendRefs for UDPRoute %s", nsn(udproute))
		return nil, false
	}

	serviceName := string(udproute.Spec.Rules[0].BackendRefs[0].Name)
	serviceNamespace := udproute.Namespace
	if udproute.Spec.Rules[0].BackendRefs[0].Namespace != nil {
		serviceNamespace = string(*udproute.Spec.Rules[0].BackendRefs[0].Namespace)
	}

	endpoints, err := k8s.CoreV1().Endpoints(serviceNamespace).Get(context.TODO(), serviceName, metav1.GetOptions{})
	if err != nil {
		log.Printf("error retrieving backendRef service %s/%s for UDPRoute %s", serviceName, serviceNamespace, nsn(udproute))
		return nil, false
	}

	if len(endpoints.Subsets) < 1 {
		log.Printf("endpoints %s/%s for UDPRoute %s had no subsets yet", serviceName, serviceNamespace, nsn(udproute))
		return nil, false
	}

	return endpoints, true
}

func addUDPRouteToLB(udproute *gatewayv1alpha2.UDPRoute, gateway *gatewayv1beta1.Gateway, listener *gatewayv1beta1.Listener) {
	endpoints, backendReady := isUDPRouteReady(udproute)
	if !backendReady {
		log.Printf("endpoints not ready for UDPRoute %s", nsn(udproute))
		return
	}

	gwip := ip2int(gateway.Status.Addresses[0].Value)
	podip := ip2int(endpoints.Subsets[0].Addresses[0].IP)

	iface, ok := router.hwaddrs[podip]
	if !ok {
		log.Printf("interface data not ready for UDPRoute %s", nsn(udproute))
		return
	}

	bpfBE := bpfBackend{
		Saddr:   gwip,
		Daddr:   podip,
		Dport:   uint16(endpoints.Subsets[0].Ports[0].Port),
		Shwaddr: iface.SrcHardwareAddr,
		Dhwaddr: iface.DestHardwareAddr,
		Nocksum: 1,
		Ifindex: iface.InterfaceIndex,
	}

	key := bpfVipKey{
		Vip:  gwip,
		Port: uint16(listener.Port),
	}

	log.Printf("adding backend for VIP %s:%d", gateway.Status.Addresses[0].Value, key.Port)

	if objs == nil || objs.Backends == nil {
		log.Printf("BPF maps not ready yet, have to wait")
		return
	}

	if err := objs.Backends.Update(key, bpfBE, ebpf.UpdateAny); err != nil {
		log.Printf("ERROR: failed to configure UDPRoute %s: %s", nsn(udproute), err)
	} else {
		log.Printf("udproute named %s created\n", udproute.Name)
	}
}

func deleteUDPRouteFromLB(udproute *gatewayv1alpha2.UDPRoute, gateway *gatewayv1beta1.Gateway, listener *gatewayv1beta1.Listener) {
	key := bpfVipKey{
		Vip:  ip2int(gateway.Status.Addresses[0].Value),
		Port: uint16(listener.Port),
	}

	if err := objs.Backends.Delete(key); err != nil {
		log.Printf("ERROR: failed to remove configuration for UDPRoute %s: %s", nsn(udproute), err)
	} else {
		log.Printf("successfully removed load-balancer configuration for UDPRoute %s", nsn(udproute))
	}
}
