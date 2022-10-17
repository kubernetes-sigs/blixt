//go:build integration_tests
// +build integration_tests

package integration

import (
	"context"
	"fmt"
	"os"
	"testing"

	"github.com/kong/kubernetes-testing-framework/pkg/clusters/addons/metallb"
	"github.com/kong/kubernetes-testing-framework/pkg/environments"
)

var (
	ctx    context.Context
	cancel context.CancelFunc
	env    environments.Environment
)

func TestMain(m *testing.M) {
	ctx, cancel = context.WithCancel(context.Background())
	defer cancel()

	var err error
	env, err = environments.NewBuilder().WithAddons(metallb.New()).Build(ctx)
	exitOnErr(err)

	exit := m.Run()

	if os.Getenv("BLIXT_TEST_KEEP_CLUSTER") != "true" {
		exitOnErr(env.Cleanup(ctx))
	}

	os.Exit(exit)
}

func exitOnErr(err error) {
	if err != nil {
		fmt.Fprintf(os.Stderr, err.Error())
		os.Exit(1)
	}
}
