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
	"bufio"
	"context"
	"fmt"
	"net"
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
	tcprouteRRKustomize     = "../../config/tests/tcproute-rr"
	tcprouteSampleName      = "blixt-tcproute-sample"
)

var tcpServerNames = []string{"blixt-tcproute-sample", "tcproute-rr-v1", "tcproute-rr-v2"}

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

	t.Log("waiting for TCP server to be available")
	require.Eventually(t, func() bool {
		server, err := env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Get(ctx, tcprouteSampleName, metav1.GetOptions{})
		require.NoError(t, err)
		return server.Status.AvailableReplicas > 0
	}, time.Minute, time.Second)

	t.Log("verifying TCP connectivity to the server")
	var conn net.Conn
	require.Eventually(t, func() bool {
		var err error
		conn, err = net.Dial("tcp", gwaddr)
		if err != nil {
			t.Logf("received error connecting to TCP server: [%s], retrying...", err)
			return false
		}
		return true
	}, time.Minute*5, time.Second)

	response := writeAndReadTCP(t, conn)
	require.Contains(t, response, tcpServerNames[0])

	t.Log("deleting the TCPRoute and verifying that TCP connection is closed")
	require.NoError(t, gwclient.GatewayV1alpha2().TCPRoutes(corev1.NamespaceDefault).Delete(ctx, tcprouteSampleName, metav1.DeleteOptions{}))
	require.Eventually(t, func() bool {
		_, err := conn.Write([]byte("blahhh\n"))
		require.NoError(t, err)

		err = conn.SetReadDeadline(time.Now().Add(time.Second * 3))
		require.NoError(t, err)
		reader := bufio.NewReader(conn)
		_, err = reader.ReadBytes(byte('\n'))
		if err != nil {
			if strings.Contains(err.Error(), "i/o timeout") {
				return true
			}
			t.Logf("received unexpected error waiting for TCPRoute to decomission: %s", err)
			return false
		}
		return false
	}, time.Minute, time.Second)
}

func TestTCPRouteRoundRobin(t *testing.T) {
	tcpRouteRRCleanupKey := "tcprouterr"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		if err := runCleanup(tcpRouteRRCleanupKey); err != nil {
			t.Errorf("cleanup failed: %s", err)
		}
	}()

	t.Log("deploying config/samples/tcproute-rr kustomize")
	require.NoError(t, clusters.KustomizeDeployForCluster(ctx, env.Cluster(), tcprouteRRKustomize))
	addCleanup(tcpRouteRRCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up config/samples/tcproute-rr kustomize")
		return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), tcprouteRRKustomize, "--ignore-not-found=true")
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

	t.Log("waiting for TCP servers to be available")
	labelSelector := metav1.LabelSelector{
		MatchExpressions: []metav1.LabelSelectorRequirement{
			{
				Key:      "app",
				Operator: metav1.LabelSelectorOpIn,
				Values:   tcpServerNames,
			},
		},
	}
	require.Eventually(t, func() bool {
		servers, err := env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).List(ctx, metav1.ListOptions{
			LabelSelector: metav1.FormatLabelSelector(&labelSelector),
		})
		require.NoError(t, err)
		for _, server := range servers.Items {
			if server.Status.AvailableReplicas <= 0 {
				return false
			}
		}
		return true
	}, time.Minute, time.Second)

	t.Log("verifying TCP connectivity to the servers")
	// We create three TCP connections, one for each backend.
	var conn1 net.Conn
	require.Eventually(t, func() bool {
		var err error
		conn1, err = net.Dial("tcp", gwaddr)
		if err != nil {
			t.Logf("received error connecting to TCP server: [%s], retrying...", err)
			return false
		}
		return true
	}, time.Minute*5, time.Second)
	conn2, err := net.Dial("tcp", gwaddr)
	require.NoError(t, err)
	conn3, err := net.Dial("tcp", gwaddr)
	require.NoError(t, err)
	conns := []net.Conn{conn1, conn2, conn3}

	// Run it twice to verify that we load balance in a round-robin fashion.
	for c := 0; c < 2; c++ {
		// We can't do names := tcpServerNames because we overwrite this in the loop later.
		var names []string
		names = append(names, tcpServerNames...)

		for _, conn := range conns {
			response := writeAndReadTCP(t, conn)
			split := strings.Split(response, ":")
			require.Len(t, split, 2)
			name := split[0]
			var removed bool
			names, removed = removeName(names, name)
			// If no name was removed from the list, it means that the response
			// does not contain the name of a known server.
			if !removed {
				t.Fatalf("received unexpected response from backend: %s", name)
			}
		}
		require.Len(t, names, 0)
	}

	t.Log("deleting the TCPRoute and verifying that all TCP connections are closed")
	require.NoError(t, gwclient.GatewayV1alpha2().TCPRoutes(corev1.NamespaceDefault).Delete(ctx, tcprouteSampleName, metav1.DeleteOptions{}))
	require.Eventually(t, func() bool {
		for _, conn := range conns {
			_, err := conn.Write([]byte("blahhh\n"))
			require.NoError(t, err)
			err = conn.SetReadDeadline(time.Now().Add(time.Second * 3))
			require.NoError(t, err)

			reader := bufio.NewReader(conn)
			_, err = reader.ReadBytes(byte('\n'))
			if err != nil {
				if strings.Contains(err.Error(), "i/o timeout") {
					continue
				}
				t.Logf("received unexpected error waiting for TCPRoute to decomission: %s", err)
			}
			return false
		}
		return true
	}, time.Minute, time.Second)
}

func removeName(names []string, name string) ([]string, bool) {
	for i, v := range names {
		if v == name {
			names = append(names[:i], names[i+1:]...)
			return names, true
		}
	}
	return nil, false
}

func writeAndReadTCP(t *testing.T, conn net.Conn) string {
	t.Helper()

	t.Logf("writing data to TCP connection with backend %s", conn.RemoteAddr().String())
	request := "wazzzaaaa"
	_, err := conn.Write([]byte(request + "\n"))
	require.NoError(t, err)

	t.Logf("reading data from TCP connection with backend %s", conn.RemoteAddr().String())
	reader := bufio.NewReader(conn)
	response, err := reader.ReadBytes(byte('\n'))
	require.NoError(t, err)
	return string(response)
}
