package integration

import (
	"time"

	. "github.com/onsi/ginkgo"
	. "github.com/onsi/gomega"
	corev1 "k8s.io/api/core/v1"
	v1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/types"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

var _ = Describe("Testing blixt controllers", func() {
	Context("Gateway Controller", func() {
		var gateway *gatewayv1beta1.Gateway
		var gatewayClass *gatewayv1beta1.GatewayClass
		var portNumber int32 = 8080
		var gatewayClassControllerName string = "konghq.com/blixt"
		BeforeEach(func() {
			gateway = &gatewayv1beta1.Gateway{
				ObjectMeta: v1.ObjectMeta{
					Name:      "test-gateway",
					Namespace: controlPlaneNamespace,
				},
				Spec: gatewayv1beta1.GatewaySpec{
					GatewayClassName: "test-gwc",
					Listeners: []gatewayv1beta1.Listener{{
						Name:     "udp",
						Protocol: gatewayv1beta1.ProtocolType(corev1.ProtocolUDP),
						Port:     gatewayv1beta1.PortNumber(portNumber),
					}},
				},
			}
			Expect(k8sClient.Create(ctx, gateway)).Should(Succeed())
			gatewayClass = &gatewayv1beta1.GatewayClass{
				ObjectMeta: v1.ObjectMeta{
					Name:      "test-gwc",
					Namespace: controlPlaneNamespace,
				},
				Spec: gatewayv1beta1.GatewayClassSpec{
					ControllerName: gatewayv1beta1.GatewayController(gatewayClassControllerName),
				},
			}
			Expect(k8sClient.Create(ctx, gatewayClass)).Should(Succeed())
		})

		// AfterEach(func() {
		// clean up
		// })

		It("Should have gateway and gatewayclass created", func() {
			Eventually(func() bool {
				err := k8sClient.Get(ctx, types.NamespacedName{
					Name: "test-gateway", Namespace: controlPlaneNamespace},
					gateway)
				return err == nil
			}, time.Second*2, time.Millisecond*300).Should(BeTrue())

			Eventually(func() bool {
				err := k8sClient.Get(ctx, types.NamespacedName{
					Name: "test-gwc", Namespace: controlPlaneNamespace},
					gatewayClass)
				return err == nil
			}, time.Second*2, time.Millisecond*300).Should(BeTrue())
		})
	})
})
