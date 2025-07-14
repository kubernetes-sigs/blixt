pub mod gateway;
pub mod gatewayclass;
pub mod tcproute;

pub use gateway::GatewayController;
pub use gateway::GatewayError;
pub use gatewayclass::GatewayClassController;
pub use tcproute::TCPRouteController;
pub use tcproute::TCPRouteError;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use crate::{K8sError, Result};

// FIXME: potentially drop pub after moving files
pub(crate) trait NamespaceName {
    fn namespace(&self) -> Result<String>;
    fn name(&self) -> Result<String>;
}

impl NamespaceName for ObjectMeta {
    fn namespace(&self) -> Result<String> {
        self.namespace
            .clone()
            .ok_or(K8sError::MissingResourceNamespace.into())
    }

    fn name(&self) -> Result<String> {
        self.name
            .clone()
            .ok_or(K8sError::MissingResourceName.into())
    }
}
