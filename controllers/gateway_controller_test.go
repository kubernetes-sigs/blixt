package controllers

import (
	"testing"

	"github.com/stretchr/testify/assert"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	"github.com/kong/blixt/internal/test/utils"
)

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
		obj              client.Object
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
