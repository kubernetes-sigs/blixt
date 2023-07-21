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
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/watch"
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

type observer interface {
	SetupReconciliation(ctx context.Context)
}

// BackendsClientManager is managing the connections and interactions with
// the available BackendsClient servers.
type BackendsClientManager struct {
	log       logr.Logger
	clientset *kubernetes.Clientset

	mu      sync.RWMutex
	clients map[string]clientInfo

	observers []observer
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
		clients:   map[string]clientInfo{},
		observers: []observer{},
	}, nil
}

// RegisterObservers adds new observer(s) to the observers list of BackendsClientManager.
// The observers will be notified whenever events happen to one of the BackendsClient.
func (c *BackendsClientManager) RegisterObservers(observers ...observer) {
	c.observers = append(c.observers, observers...)
}

// DeregisterObserver removes an observer from the observers list of BackendsClientManager.
func (c *BackendsClientManager) DeregisterObserver(o observer) {
	numObservers := len(c.observers)
	for i, currObserver := range c.observers {
		if o == currObserver {
			c.observers[numObservers-1], c.observers[i] =
				c.observers[i], c.observers[numObservers-1]
			c.observers = c.observers[:numObservers-1]
			return
		}
	}
}

// notifyObservers triggers the observer.SetupReconciliation function of each observer
// concurrently.
func (c *BackendsClientManager) notifyObservers(ctx context.Context) {
	c.log.Info("BackendsClientManager triggered observers for reconciliation")

	var wg sync.WaitGroup
	wg.Add(len(c.observers))

	for _, o := range c.observers {
		go func(o observer) {
			defer wg.Done()
			o.SetupReconciliation(ctx)
		}(o)
	}

	wg.Wait()
}

// ManageDataPlanePods watches for events regarding the system's data plane pods,
// create/destroy connections with them, and notifies the registered observers.
func (c *BackendsClientManager) ManageDataPlanePods(ctx context.Context) error {
	c.log.Info("BackendsClientManager", "status", "startup")

	listOptions := metav1.ListOptions{
		LabelSelector: fmt.Sprintf("app=%s,component=%s",
			vars.DefaultDataPlaneAppLabel, vars.DefaultDataPlaneComponentLabel),
	}

	watcher, err := c.clientset.CoreV1().Pods(vars.DefaultNamespace).Watch(ctx, listOptions)
	if err != nil {
		return err
	}

	go func() {
		for event := range watcher.ResultChan() {
			c.handleDataPlanePodEvent(ctx, event)
		}
		c.closeConnections()
	}()

	return nil
}

func (c *BackendsClientManager) handleDataPlanePodEvent(ctx context.Context, event watch.Event) {
	dataPlanePod, ok := event.Object.(*corev1.Pod)
	if !ok {
		return
	}

	key := dataPlanePod.GetNamespace() + dataPlanePod.GetName()

	switch event.Type {
	case watch.Deleted:
		c.log.Info("BackendsClientManager", "status", "disconnecting", "pod", dataPlanePod.GetName())

		c.mu.Lock()
		b, ok := c.clients[key]
		if !ok {
			return
		}
		delete(c.clients, key)
		c.mu.Unlock()

		if err := b.conn.Close(); err != nil {
			c.log.Error(err, "BackendsClientManager", "status", "disconnection failed", "pod", dataPlanePod.GetName())
			break
		}
		c.log.Info("BackendsClientManager", "status", "disconnected", "pod", dataPlanePod.GetName())

	case watch.Added, watch.Modified:

		// If no ip is configured yet, or we already established a connection, skip this event.
		_, connectionEstablished := c.clients[key]
		if dataPlanePod.Status.PodIP == "" || connectionEstablished {
			return
		}

		endpoint := fmt.Sprintf("%s:%d", dataPlanePod.Status.PodIP, vars.DefaultDataPlaneAPIPort)
		c.log.Info("BackendsClientManager", "status", "connecting", "pod", dataPlanePod.GetName(), "endpoint", endpoint)

		conn, err := grpc.DialContext(ctx, endpoint, grpc.WithTransportCredentials(insecure.NewCredentials()), grpc.WithBlock())
		if err != nil {
			c.log.Error(err, "BackendsClientManager", "status", "connection failure", "pod", dataPlanePod.GetName())
			return
		}

		c.mu.Lock()
		c.clients[key] = clientInfo{
			conn:   conn,
			client: NewBackendsClient(conn),
			name:   dataPlanePod.Name,
		}
		c.mu.Unlock()

		c.log.Info("BackendsClientManager", "status", "connected", "pod", dataPlanePod.GetName())

	default:
		return

	}

	c.notifyObservers(ctx)
}

func (c *BackendsClientManager) closeConnections() {
	c.log.Info("BackendsClientManager", "status", "shutting down")
	var wg sync.WaitGroup
	wg.Add(len(c.clients))

	for _, cc := range c.clients {
		go func(cc clientInfo) {
			defer wg.Done()
			cc.conn.Close()
		}(cc)
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
