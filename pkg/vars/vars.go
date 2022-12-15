package vars

// -----------------------------------------------------------------------------
// ControlPlane Constants
// -----------------------------------------------------------------------------

const (
	// GatewayClassControllerName is the unique identifier indicating controller
	// responsible for relevant resources.
	GatewayClassControllerName = "konghq.com/blixt"
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
