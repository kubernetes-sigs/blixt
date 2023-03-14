//go:build integration_tests
// +build integration_tests

package integration

import (
	"bytes"
	"context"
	"fmt"
	"github.com/google/uuid"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/stretchr/testify/require"
	"io"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"net"
	"os"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
	"testing"
	"time"

	testutils "github.com/kong/blixt/internal/test/utils"
)

const (
	udprouteSampleKustomize        = "../../config/tests/udproute"
	udproutenoreachSampleKustomize = "../../config/tests/udproute-noreach"
	udprouteSampleName             = "blixt-udproute-sample"
)

func TestUDPRouteBasics(t *testing.T) {
	udpRouteBasicsCleanupKey := "udproutebasics"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
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
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		runCleanup(udpRouteNoReachCleanupKey)
	}()

	t.Log("deploying config/samples/udproute-noreach kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), udproutenoreachSampleKustomize))
	addCleanup(udpRouteNoReachCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up config/samples/udproute-noreach kustomize")
		return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), udproutenoreachSampleKustomize)
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

	// ensure server sent back icmp host unreachable
	t.Logf("ensuring UDP server sent back icmp host unreachable")
	err = conn.SetReadDeadline(time.Now().Add(5 * time.Second))
	require.NoError(t, err)
	_, err = conn.Read(make([]byte, 2048))
	require.ErrorContains(t, err, "read: connection refused")
}

func TestUDPDeletionGracePeriod(t *testing.T) {
	/*
		- Check if deployment is up
		- Check if pods & service is ready
		- if yes query service for UDPRoute
			- and put delete timestamp of future
		- Update some configuration, and check if its reflecting
		- Delete
	*/
	udpRouteDeletionGracePeriodKey := "udproutedeletion"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		runCleanup(udpRouteDeletionGracePeriodKey)
	}()

	t.Log("deploying config/samples/udproute kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), udprouteSampleKustomize))
	addCleanup(udpRouteDeletionGracePeriodKey, func(ctx context.Context) error {
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
	t.Log("Received address", gwaddr)

	t.Log("waiting for udp server to be available")
	require.Eventually(t, func() bool {
		server, err := env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return server.Status.AvailableReplicas > 0
	}, time.Minute, time.Second)

	//Query UDPRoute
	t.Log("Retrieve configured udp route")
	require.Eventually(t, func() bool {
		udproute, err := gwclient.GatewayV1alpha2().UDPRoutes(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return len(udproute.Spec.Rules[0].BackendRefs) > 0
	}, time.Minute, time.Second*2)

	t.Log("Deleting UDP Route for Grace Period")
	require.Eventually(t, func() bool {

		udproute, err := gwclient.GatewayV1alpha2().UDPRoutes(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)

		// deletionTime := metav1.NewTime(time.Now().Add(30 * time.Second))
		// udproute.ObjectMeta.DeletionTimestamp = &deletionTime

		_ = gwclient.GatewayV1alpha2().UDPRoutes(corev1.NamespaceDefault).Delete(ctx, udprouteSampleName, metav1.DeleteOptions{})

		t.Log("Retrieve object again to make sure its getting recoiled")
		udproute, err = gwclient.GatewayV1alpha2().UDPRoutes(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)

		t.Log("Updating the object, with new backend ref", "Updated UDPRoute", udproute)
		portN := gatewayv1alpha2.PortNumber(9876)
		udproute.Spec.Rules[0].BackendRefs = append(udproute.Spec.Rules[0].BackendRefs, gatewayv1alpha2.BackendRef{BackendObjectReference: gatewayv1alpha2.BackendObjectReference{
			Name: "blixt-udproute-deletion-test",
			Port: &portN,
		}})

		udproute, err = gwclient.GatewayV1alpha2().UDPRoutes(corev1.NamespaceDefault).Update(ctx, udproute, metav1.UpdateOptions{})
		require.NoError(t, err)

		t.Log("Dumping retreived object", "BackendRefs", udproute.Spec.Rules[0].BackendRefs)
		return len(udproute.Spec.Rules[0].BackendRefs) > 1
	}, time.Minute, time.Second)

	// require.Eventually(t, func() bool {
	// 	udproute, err := gwclient.GatewayV1alpha2().UDPRoutes(corev1.NamespaceDefault).Get(ctx, udprouteSampleName, metav1.GetOptions{})
	// 	require.NoError(t, err)
	// 	t.Log("Recoild udproute", "route", udproute)
	// 	return len(udproute.Spec.Rules[0].BackendRefs) > 0
	// }, time.Minute, time.Second*2)
}
