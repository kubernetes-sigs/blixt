package controllers

import (
	"context"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/client-go/kubernetes/scheme"
	controllerruntimeclient "sigs.k8s.io/controller-runtime/pkg/client"
	fakectrlruntimeclient "sigs.k8s.io/controller-runtime/pkg/client/fake"
	"sigs.k8s.io/controller-runtime/pkg/reconcile"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	"github.com/kong/blixt/internal/test/utils"
	"github.com/kong/blixt/pkg/vars"
)

func init() {
	_ = gatewayv1beta1.AddToScheme(scheme.Scheme)
}

func TestGatewayReconciler_gatewayHasMatchingGatewayClass(t *testing.T) {
	logger, output := utils.NewBytesBufferLogger()
	managedGWC, unmanagedGWC, fakeClient := utils.NewFakeClientWithGatewayClasses()
	r := GatewayReconciler{
		Client: fakeClient,
		Scheme: fakeClient.Scheme(),
		Log:    logger,
	}

	for _, tt := range []struct {
		name             string
		obj              controllerruntimeclient.Object
		expected         bool
		logEntryExpected string
	}{
		{
			name: "a gateway with a gatewayclass managed by our controller name matches",
			obj: &gatewayv1beta1.Gateway{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "managed-gateway",
					Namespace: corev1.NamespaceDefault,
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: managedGWC,
				},
			},
			expected: true,
		},
		{
			name: "a gateway with a gatewayclass NOT managed by our controller name doesn't match",
			obj: &gatewayv1beta1.Gateway{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "unmanaged-gateway",
					Namespace: corev1.NamespaceDefault,
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: unmanagedGWC,
				},
			},
			expected: false,
		},
		{
			name: "a gateway with a gatewayclass which is missing doesn't match",
			obj: &gatewayv1beta1.Gateway{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "unmanaged-gateway",
					Namespace: corev1.NamespaceDefault,
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: "non-existent-gateway-class",
				},
			},
			expected:         false,
			logEntryExpected: "gatewayclass not found",
		},
		{
			name:             "if inexplicably controller-runtime feeds the predicate a non-gateway object, it doesn't match",
			obj:              &gatewayv1beta1.HTTPRoute{},
			expected:         false,
			logEntryExpected: "unexpected object type in gateway watch predicates",
		},
	} {
		obj := tt.obj
		expected := tt.expected
		logEntry := tt.logEntryExpected

		t.Run(tt.name, func(t *testing.T) {
			assert.Equal(t, expected, r.gatewayHasMatchingGatewayClass(obj))
			if logEntry == "" {
				assert.Equal(t, logEntry, output.String())
			} else {
				assert.Contains(t, output.String(), logEntry)
			}
		})

		output.Reset()
	}
}

func TestGatewayReconciler_reconcile(t *testing.T) {
	testCases := []struct {
		name         string
		gatewayReq   reconcile.Request
		gatewayClass *gatewayv1beta1.GatewayClass
		gateway      *gatewayv1beta1.Gateway
		objectsToAdd []controllerruntimeclient.Object

		run func(t *testing.T, reconciler GatewayReconciler, gatewayReq reconcile.Request, gatewayClass *gatewayv1beta1.Gateway)
	}{
		{
			name: "gatewayclass not accepted",
			gatewayReq: reconcile.Request{
				NamespacedName: types.NamespacedName{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
			},
			gatewayClass: &gatewayv1beta1.GatewayClass{
				ObjectMeta: metav1.ObjectMeta{
					Name: "test-gatewayclass",
				},
				Spec: gatewayv1beta1.GatewayClassSpec{
					ControllerName: gatewayv1beta1.GatewayController("mismatch-controller-name"),
				},
			},
			gateway: &gatewayv1beta1.Gateway{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: "test-gatewayclass",
					Listeners: []gatewayv1beta1.Listener{
						{
							Name:     "udp",
							Protocol: gatewayv1beta1.UDPProtocolType,
							Port:     9875,
						},
					},
				},
			},
			run: func(t *testing.T, reconciler GatewayReconciler, gatewayReq reconcile.Request, gateway *gatewayv1beta1.Gateway) {
				ctx := context.Background()
				_, err := reconciler.Reconcile(ctx, gatewayReq)
				require.NoError(t, err)
				newGateway := &gatewayv1beta1.Gateway{}
				err = reconciler.Client.Get(ctx, gatewayReq.NamespacedName, newGateway)
				require.NoError(t, err)
				require.Len(t, newGateway.Status.Conditions, 0)
				require.Len(t, newGateway.Status.Listeners, 0)
				require.Len(t, newGateway.Status.Addresses, 0)
			},
		},
		{
			name: "gatewayclass accepted, gateway ready",
			gatewayReq: reconcile.Request{
				NamespacedName: types.NamespacedName{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
			},
			gatewayClass: &gatewayv1beta1.GatewayClass{
				ObjectMeta: metav1.ObjectMeta{
					Name: "test-gatewayclass",
				},
				Spec: gatewayv1beta1.GatewayClassSpec{
					ControllerName: vars.GatewayClassControllerName,
				},
			},
			gateway: &gatewayv1beta1.Gateway{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: "test-gatewayclass",
					Listeners: []gatewayv1beta1.Listener{
						{
							Name:          "udp",
							Protocol:      gatewayv1beta1.UDPProtocolType,
							Port:          9875,
							AllowedRoutes: &gatewayv1beta1.AllowedRoutes{},
						},
					},
				},
			},
			objectsToAdd: []controllerruntimeclient.Object{
				&corev1.Service{
					ObjectMeta: metav1.ObjectMeta{
						Namespace: "test-namespace",
						Name:      "service-for-gateway-test-gateway",
						Labels: map[string]string{
							gatewayServiceLabel: "test-gateway",
						},
					},
					Spec: corev1.ServiceSpec{
						Type:      corev1.ServiceTypeLoadBalancer,
						ClusterIP: "1.1.1.1",
						Ports: []corev1.ServicePort{
							{
								Name:     "udp",
								Protocol: corev1.ProtocolUDP,
								Port:     9875,
							},
						},
					},
					Status: corev1.ServiceStatus{
						LoadBalancer: corev1.LoadBalancerStatus{
							Ingress: []corev1.LoadBalancerIngress{
								{
									IP: "1.2.3.4",
								},
							},
						},
					},
				},
				&corev1.Endpoints{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "service-for-gateway-test-gateway",
						Namespace: "test-namespace",
					},
				},
			},
			run: func(t *testing.T, reconciler GatewayReconciler, gatewayReq reconcile.Request, gateway *gatewayv1beta1.Gateway) {
				ctx := context.Background()
				_, err := reconciler.Reconcile(ctx, gatewayReq)
				require.NoError(t, err)
				newGateway := &gatewayv1beta1.Gateway{}
				err = reconciler.Client.Get(ctx, gatewayReq.NamespacedName, newGateway)
				require.NoError(t, err)
				require.Len(t, newGateway.Status.Addresses, 1)
				require.Len(t, newGateway.Status.Conditions, 1)
				require.Equal(t, newGateway.Status.Conditions[0].Status, metav1.ConditionTrue)
				require.Len(t, newGateway.Status.Listeners, 1)
				require.Equal(t, newGateway.Status.Listeners[0].SupportedKinds, []gatewayv1beta1.RouteGroupKind{
					{
						Group: (*gatewayv1beta1.Group)(&gatewayv1beta1.GroupVersion.Group),
						Kind:  "UDPRoute",
					},
				})
				for _, c := range newGateway.Status.Listeners[0].Conditions {
					require.Equal(t, c.Status, metav1.ConditionTrue)
				}

			},
		},
		{
			name: "gatewayclass accepted, gateway not ready because resources are missing",
			gatewayReq: reconcile.Request{
				NamespacedName: types.NamespacedName{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
			},
			gatewayClass: &gatewayv1beta1.GatewayClass{
				ObjectMeta: metav1.ObjectMeta{
					Name: "test-gatewayclass",
				},
				Spec: gatewayv1beta1.GatewayClassSpec{
					ControllerName: vars.GatewayClassControllerName,
				},
			},
			gateway: &gatewayv1beta1.Gateway{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: "test-gatewayclass",
					Listeners: []gatewayv1beta1.Listener{
						{
							Name:          "udp",
							Protocol:      gatewayv1beta1.UDPProtocolType,
							Port:          9875,
							AllowedRoutes: &gatewayv1beta1.AllowedRoutes{},
						},
					},
				},
			},
			run: func(t *testing.T, reconciler GatewayReconciler, gatewayReq reconcile.Request, gateway *gatewayv1beta1.Gateway) {
				ctx := context.Background()
				_, err := reconciler.Reconcile(ctx, gatewayReq)
				require.NoError(t, err)
				newGateway := &gatewayv1beta1.Gateway{}
				err = reconciler.Client.Get(ctx, gatewayReq.NamespacedName, newGateway)
				require.NoError(t, err)
				require.Len(t, newGateway.Status.Addresses, 0)
				require.Len(t, newGateway.Status.Conditions, 1)
				require.Equal(t, newGateway.Status.Conditions[0].Status, metav1.ConditionFalse)
				require.Len(t, newGateway.Status.Listeners, 1)
				require.Equal(t, newGateway.Status.Listeners[0].SupportedKinds, []gatewayv1beta1.RouteGroupKind{
					{
						Group: (*gatewayv1beta1.Group)(&gatewayv1beta1.GroupVersion.Group),
						Kind:  "UDPRoute",
					},
				})
				for _, c := range newGateway.Status.Listeners[0].Conditions {
					if c.Type == string(gatewayv1beta1.ListenerConditionResolvedRefs) {
						require.Equal(t, c.Status, metav1.ConditionTrue)
					} else {
						require.Equal(t, c.Status, metav1.ConditionFalse)
					}
				}
			},
		},
		{
			name: "gatewayclass accepted, gateway not ready because resolvedrefs is false",
			gatewayReq: reconcile.Request{
				NamespacedName: types.NamespacedName{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
			},
			gatewayClass: &gatewayv1beta1.GatewayClass{
				ObjectMeta: metav1.ObjectMeta{
					Name: "test-gatewayclass",
				},
				Spec: gatewayv1beta1.GatewayClassSpec{
					ControllerName: vars.GatewayClassControllerName,
				},
			},
			gateway: &gatewayv1beta1.Gateway{
				ObjectMeta: metav1.ObjectMeta{
					Name:      "test-gateway",
					Namespace: "test-namespace",
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: "test-gatewayclass",
					Listeners: []gatewayv1beta1.Listener{
						{
							Name:          "http",
							Protocol:      gatewayv1beta1.HTTPProtocolType,
							Port:          9875,
							AllowedRoutes: &gatewayv1beta1.AllowedRoutes{},
						},
						{
							Name:          "udp",
							Protocol:      gatewayv1beta1.UDPProtocolType,
							Port:          9875,
							AllowedRoutes: &gatewayv1beta1.AllowedRoutes{},
						},
					},
				},
			},
			objectsToAdd: []controllerruntimeclient.Object{
				&corev1.Service{
					ObjectMeta: metav1.ObjectMeta{
						Namespace: "test-namespace",
						Name:      "service-for-gateway-test-gateway",
						Labels: map[string]string{
							gatewayServiceLabel: "test-gateway",
						},
					},
					Spec: corev1.ServiceSpec{
						Type:      corev1.ServiceTypeLoadBalancer,
						ClusterIP: "1.1.1.1",
						Ports: []corev1.ServicePort{
							{
								Name:     "udp",
								Protocol: corev1.ProtocolUDP,
								Port:     9875,
							},
						},
					},
					Status: corev1.ServiceStatus{
						LoadBalancer: corev1.LoadBalancerStatus{
							Ingress: []corev1.LoadBalancerIngress{
								{
									IP: "1.2.3.4",
								},
							},
						},
					},
				},
				&corev1.Endpoints{
					ObjectMeta: metav1.ObjectMeta{
						Name:      "service-for-gateway-test-gateway",
						Namespace: "test-namespace",
					},
				},
			},
			run: func(t *testing.T, reconciler GatewayReconciler, gatewayReq reconcile.Request, gateway *gatewayv1beta1.Gateway) {
				ctx := context.Background()
				_, err := reconciler.Reconcile(ctx, gatewayReq)
				require.NoError(t, err)
				newGateway := &gatewayv1beta1.Gateway{}
				err = reconciler.Client.Get(ctx, gatewayReq.NamespacedName, newGateway)
				require.NoError(t, err)
				require.Len(t, newGateway.Status.Addresses, 1)
				require.Len(t, newGateway.Status.Conditions, 1)
				require.Equal(t, newGateway.Status.Conditions[0].Status, metav1.ConditionFalse)
				require.Len(t, newGateway.Status.Listeners, 2)
				for _, l := range newGateway.Status.Listeners {
					if l.Name == "http" {
						require.Len(t, l.SupportedKinds, 0)
						for _, c := range l.Conditions {
							if c.Type == string(gatewayv1beta1.ListenerConditionResolvedRefs) {
								require.Equal(t, c.Status, metav1.ConditionFalse)
							} else {
								require.Equal(t, c.Status, metav1.ConditionFalse)
							}
						}
					}
					if l.Name == "udp" {
						require.Equal(t, l.SupportedKinds, []gatewayv1beta1.RouteGroupKind{
							{
								Group: (*gatewayv1beta1.Group)(&gatewayv1beta1.GroupVersion.Group),
								Kind:  "UDPRoute",
							},
						})
						for _, c := range l.Conditions {
							require.Equal(t, c.Status, metav1.ConditionTrue)
						}
					}
				}
			},
		},
	}

	for _, tc := range testCases {
		tc := tc

		t.Run(tc.name, func(t *testing.T) {
			objectsToAdd := []controllerruntimeclient.Object{
				tc.gatewayClass,
				tc.gateway,
			}
			objectsToAdd = append(objectsToAdd, tc.objectsToAdd...)

			fakeClient := fakectrlruntimeclient.
				NewClientBuilder().
				WithScheme(scheme.Scheme).
				WithObjects(objectsToAdd...).
				Build()

			reconciler := GatewayReconciler{
				Client: fakeClient,
			}

			tc.run(t, reconciler, tc.gatewayReq, tc.gateway)
		})
	}
}
