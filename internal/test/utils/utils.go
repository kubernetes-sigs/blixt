package utils

import (
	"bytes"
	"context"
	"fmt"
	"log"
	"testing"

	"github.com/go-logr/logr"
	"github.com/go-logr/stdr"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/kong/kubernetes-testing-framework/pkg/environments"
	"github.com/stretchr/testify/assert"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	utilruntime "k8s.io/apimachinery/pkg/util/runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/client/fake"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	"github.com/kong/blixt/pkg/vars"
)

// NewBytesBufferLogger creates a standard logger with a *bytes.Buffer as the
// output wrapped in a logr.Logger implementation to provide to reconcilers.
func NewBytesBufferLogger() (logr.Logger, *bytes.Buffer) {
	output := new(bytes.Buffer)
	stdr.SetVerbosity(1)
	return stdr.NewWithOptions(log.New(output, "", log.Default().Flags()), stdr.Options{}), output
}

// NewFakeClientWithGatewayClasses creates a new fake controller-runtime
// client.Client for testing, with two GatewayClasses pre-loaded into the
// client: one that's managed by our GatewayClassControllerName, and one that
// isn't. You can also optionally provide more init objects to pre-load if
// needed.
func NewFakeClientWithGatewayClasses(initObjects ...client.Object) (gatewayv1beta1.ObjectName, gatewayv1beta1.ObjectName, client.Client) {
	managedGatewayClass := &gatewayv1beta1.GatewayClass{
		ObjectMeta: metav1.ObjectMeta{
			Name: "managed-gateway-class",
		},
		Spec: gatewayv1beta1.GatewayClassSpec{
			ControllerName: vars.GatewayClassControllerName,
		},
	}
	unmanagedGatewayClass := &gatewayv1beta1.GatewayClass{
		ObjectMeta: metav1.ObjectMeta{
			Name: "unmanaged-gateway-class",
		},
		Spec: gatewayv1beta1.GatewayClassSpec{
			ControllerName: "kubernetes.io/unmanaged",
		},
	}

	scheme := runtime.NewScheme()
	utilruntime.Must(gatewayv1beta1.AddToScheme(scheme))

	fakeClient := fake.NewClientBuilder().WithObjects(
		managedGatewayClass,
		unmanagedGatewayClass,
	).
		WithObjects(initObjects...).
		WithScheme(scheme).
		Build()

	return gatewayv1beta1.ObjectName(managedGatewayClass.Name),
		gatewayv1beta1.ObjectName(unmanagedGatewayClass.Name),
		fakeClient
}

// WaitForBlixtReadiness waits for Blixt to be ready in the provided testing
// environment (but deploying Blixt is expected to have already been handled
// elsewhere).
func WaitForBlixtReadiness(ctx context.Context, env environments.Environment) error {
	for {
		select {
		case <-ctx.Done():
			if err := ctx.Err(); err != nil {
				return fmt.Errorf("context completed while waiting for components: %w", err)
			}
			return fmt.Errorf("context completed while waiting for components")
		default:
			var controlplaneReady, dataplaneReady bool

			controlplane, err := env.Cluster().Client().AppsV1().Deployments(vars.DefaultNamespace).Get(ctx, vars.DefaultControlPlaneDeploymentName, metav1.GetOptions{})
			if err != nil {
				return err
			}
			if controlplane.Status.AvailableReplicas > 0 {
				controlplaneReady = true
			}

			dataplane, err := env.Cluster().Client().AppsV1().DaemonSets(vars.DefaultNamespace).Get(ctx, vars.DefaultDataPlaneDaemonSetName, metav1.GetOptions{})
			if err != nil {
				return err
			}
			if dataplane.Status.NumberAvailable > 0 {
				dataplaneReady = true
			}

			if controlplaneReady && dataplaneReady {
				return nil
			}
		}
	}
}
