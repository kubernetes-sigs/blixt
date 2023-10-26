//go:build integration_tests
// +build integration_tests

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

package integration

import (
	"context"
	"fmt"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/stretchr/testify/require"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	testutils "github.com/kubernetes-sigs/blixt/internal/test/utils"
)

const (
	tcprouteSampleKustomize = "../../config/tests/tcproute"
	tcprouteSampleName      = "blixt-tcproute-sample"
)

func TestTCPRouteBasics(t *testing.T) {
	tcpRouteBasicsCleanupKey := "tcproutebasics"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		if err := runCleanup(tcpRouteBasicsCleanupKey); err != nil {
			t.Errorf("cleanup failed: %s", err)
		}
	}()

	t.Log("deploying config/samples/tcproute kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), tcprouteSampleKustomize))
	addCleanup(tcpRouteBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up config/samples/tcproute kustomize")
		return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), tcprouteSampleKustomize, "--ignore-not-found=true")
	})

	t.Log("waiting for Gateway to have an address")
	var gw *gatewayv1beta1.Gateway
	require.Eventually(t, func() bool {
		var err error
		gw, err = gwclient.GatewayV1beta1().Gateways(corev1.NamespaceDefault).Get(ctx, tcprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return len(gw.Status.Addresses) > 0
	}, time.Minute, time.Second)
	require.NotNil(t, gw.Status.Addresses[0].Type)
	require.Equal(t, gatewayv1beta1.IPAddressType, *gw.Status.Addresses[0].Type)
	gwaddr := fmt.Sprintf("%s:8080", gw.Status.Addresses[0].Value)

	t.Log("waiting for HTTP server to be available")
	require.Eventually(t, func() bool {
		server, err := env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Get(ctx, tcprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return server.Status.AvailableReplicas > 0
	}, time.Minute, time.Second)

	t.Log("verifying HTTP connectivity to the server")
	httpc := http.Client{Timeout: time.Second * 30}
	require.Eventually(t, func() bool {
		resp, err := httpc.Get(fmt.Sprintf("http://%s/status/%d", gwaddr, http.StatusTeapot))
		if err != nil {
			t.Logf("received error checking HTTP server: [%s], retrying...", err)
			return false
		}
		defer resp.Body.Close()
		return resp.StatusCode == http.StatusTeapot
	}, time.Minute*5, time.Second)

	t.Log("deleting the TCPRoute and verifying that HTTP traffic stops")
	require.NoError(t, gwclient.GatewayV1alpha2().TCPRoutes(corev1.NamespaceDefault).Delete(ctx, tcprouteSampleName, metav1.DeleteOptions{}))
	httpc = http.Client{Timeout: time.Second * 3}
	require.Eventually(t, func() bool {
		resp, err := httpc.Get(fmt.Sprintf("http://%s/status/%d", gwaddr, http.StatusTeapot))
		if err != nil {
			if strings.Contains(err.Error(), "context deadline exceeded") {
				return true
			}
			t.Logf("received unexpected error waiting for TCPRoute to decomission: %s", err)
			return false
		}
		defer resp.Body.Close()
		return false
	}, time.Minute, time.Second)
}
