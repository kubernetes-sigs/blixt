// The system namespace for Blixt resources.
pub const BLIXT_NAMESPACE: &str = "blixt-system";

// The app label value to identify a Blixt resource.
pub const BLIXT_APP_LABEL: &str = "blixt";

// The component label value to identify the Blixt data-plane component.
pub const BLIXT_DATAPLANE_COMPONENT_LABEL: &str = "dataplane";

#[allow(dead_code)]
// The finalizer used for Blixt dataplane cleanup.
pub const DATAPLANE_FINALIZER: &str = "blixt.gateway.networking.k8s.io/dataplane";

// Controller name for the Blixt GatewayClass.
pub const GATEWAY_CLASS_CONTROLLER_NAME: &str = "gateway.networking.k8s.io/blixt";

// Field manager for Blixt.
pub const BLIXT_FIELD_MANAGER: &str = "blixt-field-manager";

// Label used to indicate that a Service is owned by a Blixt Gateway.
pub const GATEWAY_SERVICE_LABEL: &str = "blixt.gateway.networking.k8s.io/owned-by-gateway";
