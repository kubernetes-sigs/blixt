//go:build conformance_tests
// +build conformance_tests

package conformance

import (
	"testing"

	"github.com/google/uuid"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
	"sigs.k8s.io/gateway-api/conformance/apis/v1alpha1"
	"sigs.k8s.io/gateway-api/conformance/tests"
	"sigs.k8s.io/gateway-api/conformance/utils/suite"

	"github.com/kong/blixt/pkg/vars"
)

const (
	showDebug                  = true
	shouldCleanup              = true
	enableAllSupportedFeatures = true
)

const (
	gatewayAPICRDKustomize        = "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v0.8.1"
	conformanceTestsBaseManifests = "https://raw.githubusercontent.com/kubernetes-sigs/gateway-api/v0.8.1/conformance/base/manifests.yaml"
)

func TestGatewayConformance(t *testing.T) {
	t.Log("configuring environment for gateway conformance tests")
	c, err := client.New(env.Cluster().Config(), client.Options{})
	require.NoError(t, err)
	require.NoError(t, gatewayv1alpha2.AddToScheme(c.Scheme()))
	require.NoError(t, gatewayv1beta1.AddToScheme(c.Scheme()))

	t.Log("deploying Gateway API CRDs")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), gatewayAPICRDKustomize))

	t.Log("deploying conformance test suite base manifests")
	require.NoError(t, clusters.ApplyManifestByURL(ctx, env.Cluster(), conformanceTestsBaseManifests))

	t.Log("starting the controller manager")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), "../../config/tests/conformance/"))

	t.Log("creating GatewayClass for gateway conformance tests")
	gatewayClass := &gatewayv1beta1.GatewayClass{
		ObjectMeta: metav1.ObjectMeta{
			Name: uuid.NewString(),
		},
		Spec: gatewayv1beta1.GatewayClassSpec{
			ControllerName: vars.GatewayClassControllerName,
		},
	}
	require.NoError(t, c.Create(ctx, gatewayClass))
	t.Cleanup(func() { assert.NoError(t, c.Delete(ctx, gatewayClass)) })

	t.Log("configuring the gateway conformance test suite")
	cSuite, err := suite.NewExperimentalConformanceTestSuite(
		suite.ExperimentalConformanceOptions{
			Options: suite.Options{
				Client:               c,
				GatewayClassName:     gatewayClass.Name,
				Debug:                showDebug,
				CleanupBaseResources: shouldCleanup,
				BaseManifests:        conformanceTestsBaseManifests,
				SupportedFeatures:    suite.GatewayCoreFeatures,
				SkipTests: []string{
					// TODO: these tests are broken because they incorrectly require HTTP support
					// see https://github.com/kubernetes-sigs/gateway-api/issues/2403
					"GatewayInvalidRouteKind",
					"GatewayInvalidTLSConfiguration",
					// TODO: these tests are disabled because we don't actually support them
					// properly yet.
					"GatewayModifyListeners",
					"GatewayClassObservedGenerationBump",
					"GatewayWithAttachedRoutes",
				},
			},
			Implementation: v1alpha1.Implementation{
				Organization: "kong",
				Project:      "blixt",
				URL:          "https://github.com/kong/blixt",
				Version:      "v0.2.0",
				Contact:      []string{"https://github.com/Kong/blixt/issues/new"},
			},
		},
	)
	require.NoError(t, err)

	t.Log("executing the gateway conformance test suite")
	cSuite.Setup(t)
	cSuite.Run(t, tests.ConformanceTests)
}
