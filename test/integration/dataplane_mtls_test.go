//go:build dataplane_tests
// +build dataplane_tests

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
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"io/ioutil"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/stretchr/testify/require"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
)

var (
	CAFile         = certFile("ca.pem")
	ClientCertFile = certFile("client.pem")
	ClientKeyFile  = certFile("client-key.pem")
	ServerCertFile = certFile("server.pem")
	ServerKeyFile  = certFile("server-key.pem")
)

// TestGRPCClient tests the gRPC client against a running Docker container.
func TestGRPCClient(t *testing.T) {
	// Step 3: Wait for the Docker container to be ready
	if err := waitForContainer(containerName); err != nil {
		t.Fatalf("Container did not start successfully: %v", err)
	}

	// Step 4: Load TLS config
	clientTLSConfig, err := setupTLSConfig(TLSConfig{
		CAFile:   CAFile,
		CertFile: ClientCertFile,
		KeyFile:  ClientKeyFile,
	})
	require.NoError(t, err)

	// Step 5: Dial grpc to test hand-shake
	clientCreds := credentials.NewTLS(clientTLSConfig)
	conn, err := grpc.Dial("localhost:9874", grpc.WithTransportCredentials(clientCreds))
	require.NoError(t, err)
	defer conn.Close()
}

// Helper functions
func certFile(filename string) string {
	return filepath.Join("certs", filename)
	if dir := os.Getenv("TEST_CERTS_PATH"); dir != "" {
		return filepath.Join(dir, filename)
	}
	panic("Env var TEST_CERTS_PATH not found. Please specify path to mTLS test certs")
}

func setupTLSConfig(cfg TLSConfig) (*tls.Config, error) {
	var err error
	tlsConfig := &tls.Config{}
	if cfg.CertFile != "" && cfg.KeyFile != "" {
		tlsConfig.Certificates = make([]tls.Certificate, 1)
		tlsConfig.Certificates[0], err = tls.LoadX509KeyPair(
			cfg.CertFile,
			cfg.KeyFile,
		)
		if err != nil {
			return nil, err
		}
	}
	if cfg.CAFile != "" {
		b, err := ioutil.ReadFile(cfg.CAFile)
		if err != nil {
			return nil, err
		}
		ca := x509.NewCertPool()
		ok := ca.AppendCertsFromPEM([]byte(b))
		if !ok {
			return nil, fmt.Errorf(
				"failed to parse root certificate: %q",
				cfg.CAFile)
		}
		if cfg.Server {
			tlsConfig.ClientCAs = ca
			tlsConfig.ClientAuth = tls.RequireAndVerifyClientCert
		} else {
			tlsConfig.RootCAs = ca
		}
		tlsConfig.ServerName = cfg.ServerAddress
	}
	return tlsConfig, nil
}

type TLSConfig struct {
	CertFile      string
	KeyFile       string
	CAFile        string
	ServerAddress string
	Server        bool
}


// waitForContainer waits until the specified Docker container is running.
func waitForContainer(containerName string) error {
	for i := 0; i < 10; i++ { // Wait up to 10 seconds
		cmd := exec.Command("docker", "inspect", "-f", "{{.State.Running}}", containerName)
		output, err := cmd.Output()
		if err == nil && string(output) == "true\n" {
			print(output)
			return nil
		}
		time.Sleep(1 * time.Second)
	}
	return fmt.Errorf("container %s is not running", containerName)
}

// cleanupDockerImage removes the Docker container.
func cleanupDockerImage(containerName string) {
	cmd := exec.Command("docker", "rm", "-f", containerName)
	cmd.Run()
}
