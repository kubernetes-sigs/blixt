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
	"bytes"
	"context"
	"fmt"
	"io"
	"net"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/stretchr/testify/require"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	testutils "github.com/kubernetes-sigs/blixt/internal/test/utils"
)

const (
	udprouteSampleKustomize        = "../../config/tests/udproute"
	udprouteRRSampleKustomize      = "../../config/tests/udproute-rr"
	udproutenoreachSampleKustomize = "../../config/tests/udproute-noreach"
	udprouteSampleName             = "blixt-udproute-sample"
)

func TestUDPRouteBasics(t *testing.T) {
	udpRouteBasicsCleanupKey := "udproutebasics"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		if err := runCleanup(udpRouteBasicsCleanupKey); err != nil {
			t.Errorf("cleanup failed: %s", err)
		}
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
	bytesWritten := sendUDPPacket(t, message, gwaddr)

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

func TestUDPRouteRoundRobin(t *testing.T) {
	udpRouteBasicsCleanupKey := "udprouterr"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		if err := runCleanup(udpRouteBasicsCleanupKey); err != nil {
			t.Errorf("cleanup failed: %s", err)
		}
	}()

	t.Log("deploying config/samples/udproute-rr kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), udprouteRRSampleKustomize))
	addCleanup(udpRouteBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up config/samples/udproute-rr kustomize")
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

	pods, err := env.Cluster().Client().CoreV1().Pods(corev1.NamespaceDefault).List(ctx, metav1.ListOptions{
		LabelSelector: fmt.Sprintf("app=%s", udprouteSampleName),
	})
	require.NoError(t, err)

	var udpServerPod corev1.Pod
	// there might be pods that have been deleted but not garbage collected yet,
	// so chose the one that doesn't have a deletion timestamp set on it.
	for _, pod := range pods.Items {
		if pod.DeletionTimestamp == nil {
			udpServerPod = pod
		}
	}
	var udpPorts []int32
	for _, port := range udpServerPod.Spec.Containers[0].Ports {
		if port.Protocol == corev1.ProtocolUDP {
			udpPorts = append(udpPorts, port.ContainerPort)
		}
	}
	require.Len(t, udpPorts, 3)

	req := env.Cluster().Client().CoreV1().Pods(corev1.NamespaceDefault).GetLogs(udpServerPod.Name, &corev1.PodLogOptions{
		Follow: true,
	})
	logs, err := req.Stream(ctx)
	require.NoError(t, err)
	defer logs.Close()
	msgs := make(chan string, 1)
	errs := make(chan error, 1)
	go listenForPacketMsg(logs, msgs, errs)

	// we run the tests twice to make sure that we circle back to the first backend
	for i := 0; i < 2; i++ {
		for _, port := range udpPorts {
			t.Logf("sending a datagram to the UDP server at %s for listener port %d", gwaddr, port)
			message := uuid.NewString()
			bytesWritten := sendUDPPacket(t, message, gwaddr)
			t.Logf("%d bytes written to the UDP server, verifying receipt", bytesWritten)

			select {
			case msg := <-msgs:
				require.Contains(t, msg, fmt.Sprintf("port %d: buffer contents: %s", port, message))
			case err := <-errs:
				t.Fatalf("error occured while streaming logs: %s", err)
			}
		}
	}
}

// listenForPacketMsg reads the provided log stream and sends relevant log messages
// into the provided msgs channel.
func listenForPacketMsg(logStream io.Reader, msgs chan string, errs chan error) {
	for {
		buf := make([]byte, 2000)
		numBytes, err := logStream.Read(buf)
		if numBytes == 0 {
			continue
		}
		if err == io.EOF {
			break
		}
		if err != nil {
			errs <- err
			continue
		}
		message := string(buf[:numBytes])
		// we don't care about log messages other than the ones that mentions
		// the buffer contents.
		if strings.Contains(message, "buffer contents") {
			msgs <- message
		}
	}
}

// sendUDPPacket sends the provided msg in a UDP packet to the provided
// address. It also confirms that the server doesn't send a packet back.
func sendUDPPacket(t *testing.T, msg, gwaddr string) int {
	t.Helper()

	conn, err := net.Dial("udp", gwaddr)
	defer conn.Close()
	require.NoError(t, err)

	bytesWritten, err := conn.Write([]byte(msg))
	require.NoError(t, err)
	require.Equal(t, len(msg), bytesWritten)

	t.Logf("ensuring UDP server sent nothing back")
	err = conn.SetReadDeadline(time.Now().Add(3 * time.Second))
	require.NoError(t, err)
	_, err = conn.Read(make([]byte, 2048))
	require.ErrorIs(t, err, os.ErrDeadlineExceeded)

	return bytesWritten
}

func TestUDPRouteNoReach(t *testing.T) {
	t.Skip("TODO: temporarily skipped due to instability, see https://github.com/kubernetes-sigs/blixt/issues/104")

	udpRouteNoReachCleanupKey := "udproutenoreach"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		if err := runCleanup(udpRouteNoReachCleanupKey); err != nil {
			t.Errorf("cleanup failed: %s", err)
		}
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
