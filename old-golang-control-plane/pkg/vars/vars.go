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

package vars

// -----------------------------------------------------------------------------
// ControlPlane Constants
// -----------------------------------------------------------------------------

const (
	// GatewayClassControllerName is the unique identifier indicating controller
	// responsible for relevant resources.
	GatewayClassControllerName = "gateway.networking.k8s.io/blixt"
)

// -----------------------------------------------------------------------------
// Component Defaults
// -----------------------------------------------------------------------------

const (
	// DefaultControlPlaneDeploymentName is the name that will be used for the
	// controlplane's Deployment (by default).
	DefaultControlPlaneDeploymentName = "blixt-controlplane"

	// DefaultDataPlaneDaemonSetName is the name that will be used for the
	// dataplane's DaemonSet (by default).
	DefaultDataPlaneDaemonSetName = "blixt-dataplane"

	// DefaultNamespace is the namespace used for the controlplane and
	// dataplane components (by default).
	DefaultNamespace = "blixt-system"

	// DefaultDataPlaneAPIPort is the network port that will be used to
	// communicate with the DataPlane API (by default).
	DefaultDataPlaneAPIPort = 9874

	// DefaultDataPlaneAppLabel indicates the label value that can be used
	// to identify dataplane components (by default).
	DefaultDataPlaneAppLabel = "blixt"

	// DefaultDataPlaneComponentLabel indicates the label value that can be used
	// to identify dataplane Pods (by default).
	DefaultDataPlaneComponentLabel = "dataplane"
)
