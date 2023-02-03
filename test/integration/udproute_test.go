//go:build integration_tests
// +build integration_tests

package integration

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"net"
	"testing"
	"time"
	"os"

	"github.com/google/uuid"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/stretchr/testify/require"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
	"github.com/kong/blixt/test/utils"
)

const (
	udprouteSampleKustomize        = "../../config/tests/udproute"
	udproutenoreachSampleKustomize = "../../config/tests/udproute-noreach"
	udprouteSampleName             = "blixt-udproute-sample"
)

func TestUDPRouteBasics(t *testing.T) {
	udpRouteBasicsCleanupKey := "udproutebasics"
	defer func() {
		utils.DumpDiagnosticsIfFailed(ctx, t , env.Cluster())
		runCleanup(udpRouteBasicsCleanupKey)
	}()

	t.Log("deploying config/samples/udproute kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), udprouteSampleKustomize))
	addCleanup(udpRouteBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up config/samples/udproute kustomize")
		return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), udprouteSampleKustomize)
	})

	t.Log("waiting for Gateway to have an address")
	var gw *gatewayv1beta1.Gateway
	require.Eventually(t, func() bool {
		var err error
		gw, err = gwclient.GatewayV1beta1().Gateways(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return len(gw.Status.Addresses) > 0
	}, time.Minute, time.Second)
	require.NotNil(t, gw.Status.Addresses[0].Type)
	require.Equal(t, gatewayv1beta1.IPAddressType, *gw.Status.Addresses[0].Type)
	gwaddr := fmt.Sprintf("%s:9875", gw.Status.Addresses[0].Value)

	t.Log("waiting for udp server to be available")
	require.Eventually(t, func() bool {
		server, err := env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return server.Status.AvailableReplicas > 0
	}, time.Minute, time.Second)
	t.Logf("sending a datagram to the UDP server at %s", gwaddr)
	message := uuid.NewString()
	conn, err := net.Dial("udp", gwaddr)
	require.NoError(t, err)
	defer conn.Close()
	bytesWritten, err := conn.Write([]byte(message))
	require.NoError(t, err)
	require.Equal(t, len(message), bytesWritten)

	// ensure server sent nothing back
	t.Logf("ensuring UDP server sent nothing back")
	err = conn.SetReadDeadline(time.Now().Add(3 * time.Second))
	require.NoError(t, err)
	_, err = conn.Read(make([]byte, 2048))
	require.ErrorIs(t, err, os.ErrDeadlineExceeded)

	t.Logf("%d bytes written to the UDP server, verifying receipt", bytesWritten)
	pods, err := env.Cluster().Client().CoreV1().Pods(corev1.NamespaceDefault).List(ctx, metav1.ListOptions{LabelSelector: fmt.Sprintf("app=%s", udprouteSampleName)})
	require.NoError(t, err)
	require.Len(t, pods.Items, 1)
	udpServerPod := pods.Items[0]
	req := env.Cluster().Client().CoreV1().Pods(corev1.NamespaceDefault).GetLogs(udpServerPod.Name, &corev1.PodLogOptions{})
	logs, err := req.Stream(ctx)
	require.NoError(t, err)
	defer logs.Close()
	output := new(bytes.Buffer)
	_, err = io.Copy(output, logs)
	require.NoError(t, err)
	require.Contains(t, output.String(), message)
}

func TestUDPRouteNoReach(t *testing.T) {
	udpRouteNoReachCleanupKey := "udproutenoreach"
	defer func() {
		utils.DumpDiagnosticsIfFailed(ctx, t , env.Cluster())
		runCleanup(udpRouteNoReachCleanupKey)
	}()

	t.Log("deploying config/samples/udproute-noreach kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), udproutenoreachSampleKustomize))
	addCleanup(udpRouteNoReachCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up config/samples/udproute-noreach kustomize")
		return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), udproutenoreachSampleKustomize)
	})

	// TODO (astoycos) this currently won't work but it will with updated control-plane logic
	// add it back so we don't have to maintain a whole new kustomize config for a one line change.
	// // Update Server to ensure it's in dry run mode then wait for it to be ready
	// server, err := env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
	// require.NoError(t, err)
	// server.Spec.Template.Spec.Containers[0].Command = []string{"./udp-test-server", "--dry-run"}
	// _, err = env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Update(ctx, server, metav1.UpdateOptions{})
	// require.NoError(t, err)

	t.Log("waiting for Gateway to have an address")
	var gw *gatewayv1beta1.Gateway
	require.Eventually(t, func() bool {
		var err error
		gw, err = gwclient.GatewayV1beta1().Gateways(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return len(gw.Status.Addresses) > 0
	}, time.Minute, time.Second)
	require.NotNil(t, gw.Status.Addresses[0].Type)
	require.Equal(t, gatewayv1beta1.IPAddressType, *gw.Status.Addresses[0].Type)
	gwaddr := fmt.Sprintf("%s:9875", gw.Status.Addresses[0].Value)

	t.Log("waiting for udp server to be available")
	require.Eventually(t, func() bool {
		server, err := env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return server.Status.AvailableReplicas > 0
	}, time.Minute, time.Second)

	t.Logf("sending a datagram to the UDP server at %s", gwaddr)
	message := uuid.NewString()
	conn, err := net.Dial("udp", gwaddr)
	require.NoError(t, err)
	defer conn.Close()
	bytesWritten, err := conn.Write([]byte(message))
	require.NoError(t, err)
	require.Equal(t, len(message), bytesWritten)

	// ensure server sent back icmp host unreachable
	t.Logf("ensuring UDP server sent back icmp host unreachable")
	err = conn.SetReadDeadline(time.Now().Add(5 * time.Second))
	require.NoError(t, err)
	_, err = conn.Read(make([]byte, 2048))
	require.ErrorContains(t, err, "read: connection refused")
}
