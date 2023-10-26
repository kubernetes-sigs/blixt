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
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/stretchr/testify/require"
	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/util/intstr"
	gatewayv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gatewayv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"

	testutils "github.com/kubernetes-sigs/blixt/internal/test/utils"
	"github.com/kubernetes-sigs/blixt/pkg/vars"
)

func TestGatewayBasics(t *testing.T) {
	gatewayBasicsCleanupKey := "gatewaybasics"
	defer func() {
		testutils.DumpDiagnosticsIfFailed(ctx, t, env.Cluster())
		runCleanup(gatewayBasicsCleanupKey) //nolint:errcheck
	}()

	t.Log("deploying GatewayClass")
	gwc := &gatewayv1beta1.GatewayClass{
		ObjectMeta: metav1.ObjectMeta{
			Name: uuid.NewString(),
		},
		Spec: gatewayv1beta1.GatewayClassSpec{
			ControllerName: vars.GatewayClassControllerName,
		},
	}
	gwc, err := gwclient.GatewayV1beta1().GatewayClasses().Create(ctx, gwc, metav1.CreateOptions{})
	require.NoError(t, err)
	addCleanup(gatewayBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up gatewayclass")
		return gwclient.GatewayV1beta1().GatewayClasses().Delete(ctx, gwc.Name, metav1.DeleteOptions{})
	})

	t.Log("waiting for GatewayClass to be accepted")
	require.Eventually(t, func() bool {
		var err error
		gwc, err = gwclient.GatewayV1beta1().GatewayClasses().Get(ctx, gwc.Name, metav1.GetOptions{})
		require.NoError(t, err)
		for _, cond := range gwc.Status.Conditions {
			if cond.Type == string(gatewayv1beta1.GatewayClassConditionStatusAccepted) &&
				cond.Reason == string(gatewayv1beta1.GatewayClassReasonAccepted) &&
				cond.Status == metav1.ConditionTrue {
				return true
			}
		}
		return false
	}, time.Minute, time.Second)

	t.Log("determining an available IP address for Gateway")
	// TODO: dynamically https://github.com/Kong/blixt/issues/96
	ipAddrType := gatewayv1beta1.IPAddressType
	gwaddr := gatewayv1beta1.GatewayAddress{
		Type:  &ipAddrType,
		Value: "172.18.0.242",
	}

	t.Log("creating a Gateway with a static IP")
	listenerPort := gatewayv1beta1.PortNumber(8080)
	gw := &gatewayv1beta1.Gateway{
		ObjectMeta: metav1.ObjectMeta{
			Name: uuid.NewString(),
		},
		Spec: gatewayv1beta1.GatewaySpec{
			GatewayClassName: gatewayv1beta1.ObjectName(gwc.Name),
			Addresses:        []gatewayv1beta1.GatewayAddress{gwaddr},
			Listeners: []gatewayv1beta1.Listener{{
				Name:     "tcp",
				Protocol: gatewayv1beta1.TCPProtocolType,
				Port:     listenerPort,
			}},
		},
	}
	gw, err = gwclient.GatewayV1beta1().Gateways(corev1.NamespaceDefault).Create(ctx, gw, metav1.CreateOptions{})
	require.NoError(t, err)
	addCleanup(gatewayBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up gateway")
		return gwclient.GatewayV1beta1().Gateways(corev1.NamespaceDefault).Delete(ctx, gw.Name, metav1.DeleteOptions{})
	})

	t.Logf("verifying that the static IP %s is allocated properly", gwaddr.Value)
	require.Eventually(t, func() bool {
		var err error
		gw, err = gwclient.GatewayV1beta1().Gateways(corev1.NamespaceDefault).Get(ctx, gw.Name, metav1.GetOptions{})
		require.NoError(t, err)
		return len(gw.Status.Addresses) > 0
	}, time.Minute, time.Second)
	require.NotNil(t, gw.Status.Addresses[0].Type)
	require.Equal(t, gatewayv1beta1.IPAddressType, *gw.Status.Addresses[0].Type)
	require.Equal(t, gwaddr.Value, gw.Status.Addresses[0].Value)

	t.Log("creating a Deployment for an HTTP server to test traffic with")
	deploymentName := uuid.NewString()
	deployment := &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name: deploymentName,
			Labels: map[string]string{
				"app": deploymentName,
			},
		},
		Spec: appsv1.DeploymentSpec{
			Selector: &metav1.LabelSelector{
				MatchLabels: map[string]string{
					"app": deploymentName,
				},
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: map[string]string{
						"app": deploymentName,
					},
				},
				Spec: corev1.PodSpec{
					Containers: []corev1.Container{{
						Name:            "server",
						Image:           "ghcr.io/shaneutt/malutki",
						ImagePullPolicy: corev1.PullIfNotPresent,
						Env: []corev1.EnvVar{{
							Name:  "LISTEN_PORT",
							Value: "8080",
						}},
						Ports: []corev1.ContainerPort{{
							ContainerPort: 8080,
							Protocol:      corev1.ProtocolTCP,
						}},
					}},
				},
			},
		},
	}
	deployment, err = env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Create(ctx, deployment, metav1.CreateOptions{})
	require.NoError(t, err)
	addCleanup(gatewayBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up deployment")
		return env.Cluster().Client().AppsV1().Deployments(corev1.NamespaceDefault).Delete(ctx, deployment.Name, metav1.DeleteOptions{})
	})

	t.Log("exposing the HTTP server via a ClusterIP type Service")
	svc := &corev1.Service{
		ObjectMeta: metav1.ObjectMeta{
			Name: "integration-tests-gateway-service",
		},
		Spec: corev1.ServiceSpec{
			Selector: map[string]string{
				"app": deploymentName,
			},
			Ports: []corev1.ServicePort{{
				Name:       "tcp",
				Port:       8080,
				Protocol:   corev1.ProtocolTCP,
				TargetPort: intstr.FromInt(8080),
			}},
		},
	}
	svc, err = env.Cluster().Client().CoreV1().Services(corev1.NamespaceDefault).Create(ctx, svc, metav1.CreateOptions{})
	require.NoError(t, err)
	addCleanup(gatewayBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up service")
		return env.Cluster().Client().CoreV1().Services(corev1.NamespaceDefault).Delete(ctx, svc.Name, metav1.DeleteOptions{})
	})

	t.Log("creating a TCPRoute to the server")
	tcproute := &gatewayv1alpha2.TCPRoute{
		ObjectMeta: metav1.ObjectMeta{
			Name: uuid.NewString(),
		},
		Spec: gatewayv1alpha2.TCPRouteSpec{
			CommonRouteSpec: gatewayv1beta1.CommonRouteSpec{
				ParentRefs: []gatewayv1alpha2.ParentReference{{
					Name: gatewayv1beta1.ObjectName(gw.Name),
					Port: &listenerPort,
				}},
			},
			Rules: []gatewayv1alpha2.TCPRouteRule{{
				BackendRefs: []gatewayv1alpha2.BackendRef{{
					BackendObjectReference: gatewayv1beta1.BackendObjectReference{
						Name: gatewayv1beta1.ObjectName(svc.Name),
						Port: &listenerPort,
					},
				}},
			}},
		},
	}
	tcproute, err = gwclient.GatewayV1alpha2().TCPRoutes(corev1.NamespaceDefault).Create(ctx, tcproute, metav1.CreateOptions{})
	require.NoError(t, err)
	addCleanup(gatewayBasicsCleanupKey, func(ctx context.Context) error {
		cleanupLog("cleaning up tcproute")
		return gwclient.GatewayV1alpha2().TCPRoutes(corev1.NamespaceDefault).Delete(ctx, tcproute.Name, metav1.DeleteOptions{})
	})

	t.Log("verifying HTTP connectivity to the server")
	httpc := http.Client{Timeout: time.Second * 10}
	require.Eventually(t, func() bool {
		resp, err := httpc.Get(fmt.Sprintf("http://%s:%d/status/%d", gwaddr.Value, listenerPort, http.StatusTeapot))
		if err != nil {
			t.Logf("received error checking HTTP server: [%s], retrying...", err)
			return false
		}
		defer resp.Body.Close()
		return resp.StatusCode == http.StatusTeapot
	}, time.Minute, time.Second)

}
