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
	cleanup map[string]([]func(context.Context) error)

	gwclient *versioned.Clientset

	controlplaneImage = os.Getenv("BLIXT_CONTROLPLANE_IMAGE")
	dataplaneImage    = os.Getenv("BLIXT_DATAPLANE_IMAGE")
	udpServerImage    = os.Getenv("BLIXT_UDP_SERVER_IMAGE")

	existingCluster      = os.Getenv("BLIXT_USE_EXISTING_KIND_CLUSTER")
	keepTestCluster      = func() bool { return os.Getenv("BLIXT_TEST_KEEP_CLUSTER") == "true" || existingCluster != "" }()
	keepKustomizeDeploys = func() bool { return os.Getenv("BLIXT_TEST_KEEP_KUSTOMIZE_DEPLOYS") == "true" }()

	mainCleanupKey = "main"
)

const (
	gwCRDsKustomize = "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v0.5.1"
	testKustomize   = "../../config/tests/blixt"
)

func TestMain(m *testing.M) {
	mainCleanupKey = "main"
	defer runCleanup(mainCleanupKey)

	// check that we have a controlplane and dataplane image to use for the tests.
	// generally the runner of the tests should have built these from the latest
	// changes prior to the tests and fed them to the test suite.
	if controlplaneImage == "" || dataplaneImage == "" || udpServerImage == "" {
		exitOnErr(fmt.Errorf("BLIXT_CONTROLPLANE_IMAGE, BLIXT_DATAPLANE_IMAGE, and BLIXT_UDP_SERVER_IMAGE must be provided"))
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
		loadImages, err = loadImages.WithImage(udpServerImage)
		exitOnErr(err)

		// create the testing environment and cluster
		env, err = environments.NewBuilder().WithAddons(metallb.New(), loadImages.Build()).Build(ctx)
		exitOnErr(err)

		if !keepTestCluster {
			addCleanup(mainCleanupKey, func(context.Context) error {
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
		addCleanup(mainCleanupKey, func(context.Context) error {
			cleanupLog("cleaning up Gateway API CRDs")
			return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), gwCRDsKustomize)
		})
	}

	// deploy the blixt controlplane and dataplane, rbac permissions, e.t.c.
	// this is what the tests will actually run against.
	fmt.Println("INFO: deploying blixt via config/test kustomize")
	exitOnErr(clusters.KustomizeDeployForCluster(ctx, env.Cluster(), testKustomize))
	if !keepKustomizeDeploys {
		addCleanup(mainCleanupKey, func(context.Context) error {
			cleanupLog("cleaning up blixt via config/test kustomize")
			return clusters.KustomizeDeleteForCluster(ctx, env.Cluster(), testKustomize)
		})
	}
	exitOnErr(waitForBlixtReadiness(ctx, env))

	exit := m.Run()

	exitOnErr(runCleanup(mainCleanupKey))
	
	os.Exit(exit)
}

func exitOnErr(err error) {
	if err == nil {
		return
	}

	if cleanupErr := runCleanup(mainCleanupKey); cleanupErr != nil {
		err = fmt.Errorf("%s; %w", err, cleanupErr)
	}

	if err != nil {
		fmt.Fprint(os.Stderr, err.Error())
		os.Exit(1)
	}
}

func addCleanup(cleanupKey string, job func(context.Context) error) {
	//initialize cleanup map if needed
	if cleanup == nil {
		cleanup = map[string]([]func(context.Context) error){cleanupKey: []func(context.Context) error{job}}
		return
	}

	//initialize cleanup entry if needed
	if _, ok := cleanup[cleanupKey]; !ok {
		cleanup[cleanupKey] = []func(context.Context) error{job}
		return
	}

	// prepend so that cleanup runs in reverse order
	cleanup[cleanupKey] = append([]func(context.Context) error{job}, cleanup[cleanupKey]...)
}

func cleanupLog(msg string, args ...any) {
	fmt.Printf(fmt.Sprintf("INFO: %s\n", msg), args...)
}

func runCleanup(cleanupKey string) (cleanupErr error) {
	if len(cleanup) < 1 {
		return
	}

	fmt.Printf("INFO: running cleanup jobs for key %s\n", cleanupKey)
	cleanupList := cleanup[cleanupKey]

	for _, job := range cleanupList {
		if err := job(ctx); err != nil {
			cleanupErr = fmt.Errorf("%s; %w", err, cleanupErr)
		}
	}
	delete(cleanup, cleanupKey)
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
