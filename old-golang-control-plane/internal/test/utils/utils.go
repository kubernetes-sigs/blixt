/*
Copyright 2023 The Kubernetes Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

package utils

import (
	"bytes"
	"context"
	"fmt"
	"log"
	"testing"
	"time"

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

	"github.com/kubernetes-sigs/blixt/pkg/vars"
)

const BlixtReadinessTimeout = time.Minute * 7

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
	ticker := time.NewTicker(BlixtReadinessTimeout)
	for {
		select {
		case <-ticker.C:
			fmt.Printf("ERROR: timed out waiting for blixt readiness for cluster %s. dumping diagnostics\n", env.Cluster().Name())
			dir, err := env.Cluster().DumpDiagnostics(ctx, "wait-for-blixt-readiness-timeout")
			if err != nil {
				return fmt.Errorf("error after timeout waiting for blixt components when trying to dump diagnostics: %w", err)
			}
			return fmt.Errorf("timeout waiting for blixt components exceeded, diagnostics dumped to %s", dir)
		case <-ctx.Done():
			if err := ctx.Err(); err != nil {
				return fmt.Errorf("context completed while waiting for components: %w", err)
			}
			dir, diagErr := env.Cluster().DumpDiagnostics(ctx, "wait-for-blixt-readiness-context-completed")
			if diagErr != nil {
				return fmt.Errorf("error after timeout waiting for blixt components when trying to dump diagnostics: %w", diagErr)
			}
			return fmt.Errorf("context completed while waiting for components, diagnostics dumped to %s", dir)
		default:
			var controlplaneReady, dataplaneReady bool

			controlplane, err := env.Cluster().Client().AppsV1().Deployments(vars.DefaultNamespace).Get(ctx, vars.DefaultControlPlaneDeploymentName, metav1.GetOptions{})
			if err != nil {
				fmt.Printf("Error while checking controlplane components: %s\n", err)
				return err
			}
			if controlplane.Status.AvailableReplicas > 0 {
				controlplaneReady = true
			}

			dataplane, err := env.Cluster().Client().AppsV1().DaemonSets(vars.DefaultNamespace).Get(ctx, vars.DefaultDataPlaneDaemonSetName, metav1.GetOptions{})
			if err != nil {
				fmt.Printf("Error while checking dataplane components: %s\n", err)
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

// DumpDiagnosticsIfFailed dumps the diagnostics if the test failed.
func DumpDiagnosticsIfFailed(ctx context.Context, t *testing.T, clusters clusters.Cluster) {
	t.Helper()

	if t.Failed() {
		output, err := clusters.DumpDiagnostics(ctx, t.Name())
		t.Logf("%s failed, dumped diagnostics to %s", t.Name(), output)
		assert.NoError(t, err)
	}
}
