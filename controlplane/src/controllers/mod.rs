use crate::{Error, Result};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

// TODO: drop pub after moving files
pub(crate) trait NamespaceName {
    fn namespace(&self) -> Result<String>;
    fn name(&self) -> Result<String>;
}

impl NamespaceName for ObjectMeta {
    fn namespace(&self) -> Result<String> {
        self.namespace
            .clone()
            .ok_or(Error::InvalidConfigError("missing namespace".to_string()))
    }

    fn name(&self) -> Result<String> {
        self.name
            .clone()
            .ok_or(Error::InvalidConfigError("missing name".to_string()))
    }
}
