//go:build performance_tests
// +build performance_tests

package performance

import (
	"context"
	"fmt"
	"os"
	"testing"

	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters/addons/loadimage"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters/addons/metallb"
	"github.com/kong/kubernetes-testing-framework/pkg/environments"
	"k8s.io/apiextensions-apiserver/pkg/client/clientset/clientset"
	"sigs.k8s.io/gateway-api/pkg/client/clientset/versioned"

	testutils "github.com/kong/blixt/internal/test/utils"
)

var (
	ctx     context.Context
	cancel  context.CancelFunc
	env     environments.Environment
	cleanup map[string]([]func(context.Context) error)

	gwclient  *versioned.Clientset
	k8sclient *clientset.Clientset

	controlplaneImage = os.Getenv("BLIXT_CONTROLPLANE_IMAGE")
	dataplaneImage    = os.Getenv("BLIXT_DATAPLANE_IMAGE")
)

const (
	gwCRDsKustomize = "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v0.8.1"
	testKustomize   = "../../config/tests/performance"
)

func TestMain(m *testing.M) {
	if controlplaneImage == "" || dataplaneImage == "" {
		exitOnErr(fmt.Errorf("BLIXT_CONTROLPLANE_IMAGE and BLIXT_DATAPLANE_IMAGE must be provided"))
	}

	ctx, cancel = context.WithCancel(context.Background())
	defer cancel()

	fmt.Println("INFO: creating a new kind cluster")
	loadImages, err := loadimage.NewBuilder().WithImage(controlplaneImage)
	exitOnErr(err)
	loadImages, err = loadImages.WithImage(dataplaneImage)
	exitOnErr(err)

	env, err = environments.NewBuilder().WithAddons(metallb.New(), loadImages.Build()).Build(ctx)
	exitOnErr(err)

	fmt.Printf("INFO: new kind cluster %s was created\n", env.Cluster().Name())
	gwclient, err = versioned.NewForConfig(env.Cluster().Config())
	exitOnErr(err)
	k8sclient, err = clientset.NewForConfig(env.Cluster().Config())
	exitOnErr(err)

	fmt.Println("INFO: deploying Gateway API CRDs")
	exitOnErr(clusters.KustomizeDeployForCluster(ctx, env.Cluster(), gwCRDsKustomize))

	fmt.Println("INFO: deploying blixt via config/test kustomize")
	exitOnErr(clusters.KustomizeDeployForCluster(ctx, env.Cluster(), testKustomize))
	exitOnErr(testutils.WaitForBlixtReadiness(ctx, env))

	fmt.Println("INFO: running performance tests")
	exit := m.Run()

	exitOnErr(env.Cluster().Cleanup(ctx))

	os.Exit(exit)
}

func exitOnErr(err error) {
	if err != nil {
		fmt.Fprint(os.Stderr, err.Error())
		os.Exit(1)
	}
}
