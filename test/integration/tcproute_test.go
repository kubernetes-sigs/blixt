//go:build integration_tests
// +build integration_tests

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

	testutils "github.com/kong/blixt/internal/test/utils"
)

const (
	tcprouteSampleKustomize = "../../config/tests/tcproute"
	tcprouteSampleName      = "blixt-tcproute-sample"
)

func TestTCPRouteBasics(t *testing.T) {
	tcpRouteBasicsCleanupKey := "tcproutebasics"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		runCleanup(tcpRouteBasicsCleanupKey)
	}()

	t.Log("deploying config/samples/tcproute kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), tcprouteSampleKustomize))
	addCleanup(tcpRouteBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up config/samples/tcproute kustomize")
		return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), tcprouteSampleKustomize)
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
	httpc := http.Client{Timeout: time.Second * 10}
	require.Eventually(t, func() bool {
		resp, err := httpc.Get(fmt.Sprintf("http://%s/status/%d", gwaddr, http.StatusTeapot))
		if err != nil {
			t.Logf("received error checking HTTP server: [%s], retrying...", err)
			return false
		}
		defer resp.Body.Close()
		return resp.StatusCode == http.StatusTeapot
	}, time.Minute, time.Second)

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
