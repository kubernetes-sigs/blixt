//go:build integration_tests
// +build integration_tests

package integration

import (
	"context"
	"fmt"
	"os"
	"testing"

	"github.com/kong/kubernetes-testing-framework/pkg/clusters"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters/addons/loadimage"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters/addons/metallb"
	"github.com/kong/kubernetes-testing-framework/pkg/clusters/types/kind"
	"github.com/kong/kubernetes-testing-framework/pkg/environments"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/gateway-api/pkg/client/clientset/versioned"

	"github.com/kong/blixt/pkg/vars"
)

var (
	ctx    context.Context
	cancel context.CancelFunc
	env    environments.Environment

	gwclient *versioned.Clientset

	controlplaneImage = os.Getenv("BLIXT_CONTROLPLANE_IMAGE")
	dataplaneImage    = os.Getenv("BLIXT_DATAPLANE_IMAGE")

	existingCluster      = os.Getenv("BLIXT_USE_EXISTING_KIND_CLUSTER")
	keepTestCluster      = func() bool { return os.Getenv("BLIXT_TEST_KEEP_CLUSTER") == "true" || existingCluster != "" }()
	keepKustomizeDeploys = func() bool { return os.Getenv("BLIXT_TEST_KEEP_KUSTOMIZE_DEPLOYS") == "true" }()

	cleanup = []func(context.Context) error{}
)

const (
	gwCRDsKustomize = "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v0.5.1"
	testKustomize   = "../../config/tests"
)

func TestMain(m *testing.M) {
	// check that we have a controlplane and dataplane image to use for the tests.
	// generally the runner of the tests should have built these from the latest
	// changes prior to the tests and fed them to the test suite.
	if controlplaneImage == "" || dataplaneImage == "" {
		exitOnErr(fmt.Errorf("BLIXT_CONTROLPLANE_IMAGE and BLIXT_DATAPLANE_IMAGE must be provided"))
	}

	ctx, cancel = context.WithCancel(context.Background())
	defer cancel()

	if existingCluster != "" {
		fmt.Printf("INFO: existing kind cluster %s was provided\n", existingCluster)

		// if an existing cluster was provided, build a test env out of that instead
		cluster, err := kind.NewFromExisting(existingCluster)
		exitOnErr(err)
		env, err = environments.NewBuilder().WithExistingCluster(cluster).Build(ctx)
		exitOnErr(err)
	} else {
		fmt.Println("INFO: creating a new kind cluster")

		// to use the provided controlplane and dataplane images we will need to add
		// them as images to load in the test cluster via an addon.
		loadImages, err := loadimage.NewBuilder().WithImage(controlplaneImage)
		exitOnErr(err)
		loadImages, err = loadImages.WithImage(dataplaneImage)
		exitOnErr(err)

		// create the testing environment and cluster
		env, err = environments.NewBuilder().WithAddons(metallb.New(), loadImages.Build()).Build(ctx)
		exitOnErr(err)

		if !keepTestCluster {
			addCleanup(func(context.Context) error {
				cleanupLog("cleaning up test environment and cluster %s\n", env.Cluster().Name())
				return env.Cleanup(ctx)
			})
		}

		fmt.Printf("INFO: new kind cluster %s was created\n", env.Cluster().Name())
	}

	// create clients that are wanted for tests
	var err error
	gwclient, err = versioned.NewForConfig(env.Cluster().Config())
	exitOnErr(err)

	// deploy the Gateway API CRDs
	fmt.Println("INFO: deploying Gateway API CRDs")
	exitOnErr(clusters.KustomizeDeployForCluster(ctx, env.Cluster(), gwCRDsKustomize))
	if !keepKustomizeDeploys {
		addCleanup(func(context.Context) error {
			cleanupLog("cleaning up Gateway API CRDs")
			return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), gwCRDsKustomize)
		})
	}

	// deploy the blixt controlplane and dataplane, rbac permissions, e.t.c.
	// this is what the tests will actually run against.
	fmt.Println("INFO: deploying blixt via config/test kustomize")
	exitOnErr(clusters.KustomizeDeployForCluster(ctx, env.Cluster(), testKustomize))
	if !keepKustomizeDeploys {
		addCleanup(func(context.Context) error {
			cleanupLog("cleaning up blixt via config/test kustomize")
			return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), testKustomize)
		})
	}
	exitOnErr(waitForBlixtReadiness(ctx, env))

	exit := m.Run()

	exitOnErr(runCleanup())

	os.Exit(exit)
}

func exitOnErr(err error) {
	if err == nil {
		return
	}

	if cleanupErr := runCleanup(); cleanupErr != nil {
		err = fmt.Errorf("%s; %w", err, cleanupErr)
	}

	if err != nil {
		fmt.Fprint(os.Stderr, err.Error())
		os.Exit(1)
	}
}

func addCleanup(job func(context.Context) error) {
	// prepend so that cleanup runs in reverse order
	cleanup = append([]func(context.Context) error{job}, cleanup...)
}

func cleanupLog(msg string, args ...any) {
	fmt.Printf(fmt.Sprintf("INFO: %s\n", msg), args...)
}

func runCleanup() (cleanupErr error) {
	if len(cleanup) < 1 {
		return
	}

	fmt.Println("INFO: running cleanup jobs")
	for _, job := range cleanup {
		if err := job(ctx); err != nil {
			cleanupErr = fmt.Errorf("%s; %w", err, cleanupErr)
		}
	}
	cleanup = nil
	return
}

func waitForBlixtReadiness(ctx context.Context, env environments.Environment) error {
	for {
		select {
		case <-ctx.Done():
			if err := ctx.Err(); err != nil {
				return fmt.Errorf("context completed while waiting for components: %w", err)
			}
			return fmt.Errorf("context completed while waiting for components")
		default:
			var controlplaneReady, dataplaneReady bool

			controlplane, err := env.Cluster().Client().AppsV1().Deployments(vars.DefaultNamespace).Get(ctx, vars.DefaultControlPlaneDeploymentName, metav1.GetOptions{})
			if err != nil {
				return err
			}
			if controlplane.Status.AvailableReplicas > 0 {
				controlplaneReady = true
			}

			dataplane, err := env.Cluster().Client().AppsV1().DaemonSets(vars.DefaultNamespace).Get(ctx, vars.DefaultDataPlaneDaemonSetName, metav1.GetOptions{})
			if err != nil {
				return err
			}
			if dataplane.Status.NumberAvailable > 0 {
				dataplaneReady = true
			}

			if controlplaneReady && dataplaneReady {
				return nil
			}
		}
	}
}
