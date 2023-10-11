//go:build conformance_tests
// +build conformance_tests

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

package conformance

import (
	"context"
	"fmt"
	"os"
	"testing"

	"github.com/kong/kubernetes-testing-framework/pkg/clusters/addons/loadimage"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters/addons/metallb"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters/types/kind"
	"github.com/kong/kubernetes-testing-framework/pkg/environments"
)

var (
	ctx context.Context
	env environments.Environment

	controlplaneImage = os.Getenv("BLIXT_CONTROLPLANE_IMAGE")
	dataplaneImage    = os.Getenv("BLIXT_DATAPLANE_IMAGE")
	udpServerImage    = os.Getenv("BLIXT_UDP_SERVER_IMAGE")

	useExistingCluster = os.Getenv("BLIXT_USE_EXISTING_CLUSTER")
)

func TestMain(m *testing.M) {
	var cancel context.CancelFunc
	ctx, cancel = context.WithCancel(context.Background())
	defer cancel()

	if useExistingCluster != "" {
		fmt.Printf("INFO: using existing kind cluster %s for test environment\n", useExistingCluster)
		cluster, err := kind.NewFromExisting(useExistingCluster)
		exitOnErr(err)
		env, err = environments.NewBuilder().WithExistingCluster(cluster).Build(ctx)
		exitOnErr(err)
	} else {
		fmt.Println("INFO: loading custom images for conformance tests")
		imageLoader, err := loadimage.NewBuilder().WithImage(controlplaneImage)
		exitOnErr(err)
		imageLoader, err = imageLoader.WithImage(dataplaneImage)
		exitOnErr(err)
		imageLoader, err = imageLoader.WithImage(udpServerImage)
		exitOnErr(err)

		fmt.Println("INFO: building the test environment and cluster")
		env, err = environments.NewBuilder().WithAddons(metallb.New(), imageLoader.Build()).Build(ctx)
		exitOnErr(err)
		addCleanup(env.Cleanup)
	}

	fmt.Println("INFO: waiting for testing environment to be ready")
	exitOnErr(<-env.WaitForReady(ctx))

	fmt.Println("INFO: running tests")
	code := m.Run()
	os.Exit(code)
}

func exitOnErr(err error) {
	if err != nil {
		fmt.Println(err.Error())
		if cleanupErr := runCleanup(); cleanupErr != nil {
			fmt.Printf("ERROR: failed during cleanup: %v", cleanupErr)
			os.Exit(2)
		}
		os.Exit(1)
	}
}

var cleanupJobs []func(ctx context.Context) error

func addCleanup(job func(ctx context.Context) error) {
	cleanupJobs = append(cleanupJobs, job)
}

func runCleanup() error {
	for _, job := range cleanupJobs {
		if err := job(ctx); err != nil {
			return err
		}
	}
	return nil
}
