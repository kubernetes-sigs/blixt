//go:build conformance_tests
// +build conformance_tests

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

package conformance

import (
	"testing"

	"github.com/go-logr/logr"
	"github.com/google/uuid"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/log"
	gatewayv1 "sigs.k8s.io/gateway-api/apis/v1"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	v1 "sigs.k8s.io/gateway-api/conformance/apis/v1"
	"sigs.k8s.io/gateway-api/conformance/tests"
	"sigs.k8s.io/gateway-api/conformance/utils/suite"

	"github.com/kubernetes-sigs/blixt/pkg/vars"
)

const (
	showDebug                  = true
	shouldCleanup              = true
	enableAllSupportedFeatures = true
)

const (
	gatewayAPICRDKustomize        = "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v1.1.0"
	conformanceTestsBaseManifests = "https://raw.githubusercontent.com/kubernetes-sigs/gateway-api/v1.1.0/conformance/base/manifests.yaml"
)

func TestGatewayConformance(t *testing.T) {
	t.Log("configuring environment for gateway conformance tests")
	log.SetLogger(logr.FromContextOrDiscard(ctx))
	c, err := client.New(env.Cluster().Config(), client.Options{})
	require.NoError(t, err)
	require.NoError(t, gatewayv1alpha2.AddToScheme(c.Scheme()))
	require.NoError(t, gatewayv1.AddToScheme(c.Scheme()))

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
	supportedFeatures := suite.GatewayCoreFeatures.Clone()
	supportedFeatures.Insert(suite.SupportGatewayStaticAddresses)
	cSuite, err := suite.NewExperimentalConformanceTestSuite(
		suite.ExperimentalConformanceOptions{
			Options: suite.Options{
				Client:               c,
				GatewayClassName:     gatewayClass.Name,
				Debug:                showDebug,
				CleanupBaseResources: shouldCleanup,
				BaseManifests:        conformanceTestsBaseManifests,
				SupportedFeatures:    supportedFeatures,
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
				UsableNetworkAddresses:   []gatewayv1beta1.GatewayAddress{{Value: "172.18.0.242"}},
				UnusableNetworkAddresses: []gatewayv1beta1.GatewayAddress{{Value: "1.1.1.1"}},
			},
			Implementation: v1.Implementation{
				Organization: "kubernetes-sigs",
				Project:      "blixt",
				URL:          "https://github.com/kubernetes-sigs/blixt",
				Version:      "v0.2.0",
				Contact:      []string{"https://github.com/kubernetes-sigs/blixt/issues/new"},
			},
		},
	)
	require.NoError(t, err)

	t.Log("executing the gateway conformance test suite")
	cSuite.Setup(t)
	cSuite.Run(t, tests.ConformanceTests) //nolint:errcheck
}
