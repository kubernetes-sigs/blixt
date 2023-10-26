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

package client

import (
	"context"
	"fmt"

	"google.golang.org/grpc"
	corev1 "k8s.io/api/core/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"

	"github.com/kubernetes-sigs/blixt/pkg/vars"
)

// NewDataPlaneClient provides a new client for communicating with the grpc API
// of the data-plane given a function which can provide the API endpoint.
func NewDataPlaneClient(ctx context.Context, c client.Client) (BackendsClient, error) {
	endpoints, err := GetDataPlaneEndpointsFromDefaultPods(ctx, c)
	if err != nil {
		return nil, err
	}

	if len(endpoints) < 1 {
		return nil, fmt.Errorf("no endpoints could be found for the dataplane API")
	}

	if len(endpoints) > 1 {
		return nil, fmt.Errorf("TODO: multiple endpoints not currently supported")
	}

	endpoint := endpoints[0]
	// TODO: mTLS https://github.com/Kong/blixt/issues/50
	conn, err := grpc.Dial(endpoint, grpc.WithInsecure(), grpc.WithBlock()) //nolint:staticcheck
	if err != nil {
		return nil, err
	}

	client := NewBackendsClient(conn)

	return client, nil
}

// GetDataPlaneEndpointsFromDefaultPods provides a list of endpoints for the
// dataplane API assuming all the default deployment settings (e.g., namespace,
// API port, e.t.c.).
func GetDataPlaneEndpointsFromDefaultPods(ctx context.Context, c client.Client) (endpoints []string, err error) {
	pods := new(corev1.PodList)
	if err = c.List(context.Background(), pods, client.MatchingLabels{
		"app":       vars.DefaultDataPlaneAppLabel,
		"component": vars.DefaultDataPlaneComponentLabel,
	}, client.InNamespace(vars.DefaultNamespace)); err != nil {
		return
	}

	for _, pod := range pods.Items {
		if pod.Status.PodIP == "" {
			err = fmt.Errorf("pod %s/%s doesn't have an IP yet", pod.Namespace, pod.Name)
			return
		}

		newEndpoint := fmt.Sprintf("%s:%d", pod.Status.PodIP, vars.DefaultDataPlaneAPIPort)
		endpoints = append(endpoints, newEndpoint)
	}

	return
}
