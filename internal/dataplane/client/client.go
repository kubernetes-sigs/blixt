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
	"errors"
	"fmt"
	"sync"

	"github.com/go-logr/logr"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/rest"
	"sigs.k8s.io/controller-runtime/pkg/log"

	"github.com/kubernetes-sigs/blixt/pkg/vars"
)

// clientInfo encapsulates the gathered information about a BackendsClient
// along with the gRPC client connection.
type clientInfo struct {
	conn   *grpc.ClientConn
	client BackendsClient
	name   string
}

// BackendsClientManager is managing the connections and interactions with
// the available BackendsClient servers.
type BackendsClientManager struct {
	log       logr.Logger
	clientset *kubernetes.Clientset

	mu      sync.RWMutex
	clients map[types.NamespacedName]clientInfo
}

// NewBackendsClientManager returns an initialized instance of BackendsClientManager.
func NewBackendsClientManager(config *rest.Config) (*BackendsClientManager, error) {
	clientset, err := kubernetes.NewForConfig(config)
	if err != nil {
		return nil, err
	}

	return &BackendsClientManager{
		log:       log.FromContext(context.Background()),
		clientset: clientset,
		mu:        sync.RWMutex{},
		clients:   map[types.NamespacedName]clientInfo{},
	}, nil
}

func (c *BackendsClientManager) SetClientsList(readyPods map[types.NamespacedName]corev1.Pod) (bool, error) {
	// TODO: close and connect to the different clients concurrently.
	clientListUpdated := false
	var err error

	// Remove old clients
	for nn, backendInfo := range c.clients {
		if _, ok := readyPods[nn]; !ok {
			c.mu.Lock()
			delete(c.clients, nn)
			c.mu.Unlock()

			if closeErr := backendInfo.conn.Close(); closeErr != nil {
				err = errors.Join(err, closeErr)
				continue
			}
			clientListUpdated = true
		}
	}

	// Add new clients
	for _, pod := range readyPods {
		key := types.NamespacedName{Namespace: pod.Namespace, Name: pod.Name}
		if _, ok := c.clients[key]; !ok {

			if pod.Status.PodIP == "" {
				continue
			}

			endpoint := fmt.Sprintf("%s:%d", pod.Status.PodIP, vars.DefaultDataPlaneAPIPort)
			c.log.Info("BackendsClientManager", "status", "connecting", "pod", pod.GetName(), "endpoint", endpoint)

			conn, dialErr := grpc.NewClient(endpoint, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithBlock())
			if dialErr != nil {
				c.log.Error(dialErr, "BackendsClientManager", "status", "connection failure", "pod", pod.GetName())
				err = errors.Join(err, dialErr)
				continue
			}

			c.mu.Lock()
			c.clients[key] = clientInfo{
				conn:   conn,
				client: NewBackendsClient(conn),
				name:   pod.Name,
			}
			c.mu.Unlock()

			c.log.Info("BackendsClientManager", "status", "connected", "pod", pod.GetName())

			clientListUpdated = true
		}
	}

	return clientListUpdated, err
}

func (c *BackendsClientManager) Close() {
	c.log.Info("BackendsClientManager", "status", "shutting down")

	c.mu.Lock()
	defer c.mu.Unlock()

	var wg sync.WaitGroup
	wg.Add(len(c.clients))

	for key, cc := range c.clients {
		go func(cc clientInfo) {
			defer wg.Done()
			cc.conn.Close()
		}(cc)

		delete(c.clients, key)
	}

	wg.Wait()

	c.log.Info("BackendsClientManager", "status", "shutdown completed")
}

func (c *BackendsClientManager) getClientsInfo() []clientInfo {
	c.mu.RLock()
	defer c.mu.RUnlock()

	backends := make([]clientInfo, 0, len(c.clients))
	for _, backendClient := range c.clients {
		backends = append(backends, backendClient)
	}

	return backends
}

// Update sends an update request to all available BackendsClient servers concurrently.
func (c *BackendsClientManager) Update(ctx context.Context, in *Targets, opts ...grpc.CallOption) (*Confirmation, error) {
	clientsInfo := c.getClientsInfo()

	var wg sync.WaitGroup
	wg.Add(len(clientsInfo))

	errs := make(chan error, len(clientsInfo))

	for _, ci := range clientsInfo {
		go func(ci clientInfo) {
			defer wg.Done()

			conf, err := ci.client.Update(ctx, in, opts...)
			if err != nil {
				c.log.Error(err, "BackendsClientManager", "operation", "update", "pod", ci.name)
				errs <- err
				return
			}
			c.log.Info("BackendsClientManager", "operation", "update", "pod", ci.name, "confirmation", conf.Confirmation)
		}(ci)
	}

	wg.Wait()
	close(errs)

	var err error
	for e := range errs {
		err = errors.Join(err, e)
	}

	return nil, err
}

// Delete sends an delete request to all available BackendsClient servers concurrently.
func (c *BackendsClientManager) Delete(ctx context.Context, in *Vip, opts ...grpc.CallOption) (*Confirmation, error) {
	clientsInfo := c.getClientsInfo()

	var wg sync.WaitGroup
	wg.Add(len(clientsInfo))

	errs := make(chan error, len(clientsInfo))

	for _, ci := range clientsInfo {
		go func(ci clientInfo) {
			defer wg.Done()

			conf, err := ci.client.Delete(ctx, in, opts...)
			if err != nil {
				c.log.Error(err, "BackendsClientManager", "operation", "delete", "pod", ci.name)
				errs <- err
				return
			}
			c.log.Info("BackendsClientManager", "operation", "delete", "pod", ci.name, "confirmation", conf.Confirmation)

		}(ci)
	}

	wg.Wait()
	close(errs)

	var err error
	for e := range errs {
		err = errors.Join(err, e)
	}

	return nil, err
}
