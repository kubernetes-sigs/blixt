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

	"github.com/google/uuid"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/stretchr/testify/require"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

const (
	udprouteSampleKustomize = "../../config/samples/udproute"
	udprouteSampleName      = "blixt-udproute-sample"
)

func TestUDPRouteBasics(t *testing.T) {
	t.Log("deploying config/samples/udproute kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), udprouteSampleKustomize))
	addCleanup(func(ctx context.Context) error {
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
