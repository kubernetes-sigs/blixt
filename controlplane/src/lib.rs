/*
Copyright 2024 The Kubernetes Authors.

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

pub mod client_manager;
mod consts;
mod gateway_controller;
mod gateway_utils;
mod gatewayclass_controller;
mod gatewayclass_utils;
mod route_utils;
mod tcproute_controller;
mod traits;
mod udproute_controller;

use std::fmt::{Debug, Display, Formatter};

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Client;
use thiserror::Error;

pub use gateway_controller::controller as gateway_controller;
pub use gatewayclass_controller::controller as gatewayclass_controller;
pub use tcproute_controller::controller as tcproute_controller;
pub use udproute_controller::controller as udproute_controller;

// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("kube error: {0}")]
    KubeError(#[from] kube::Error),
    #[error("invalid configuration: `{0}`")]
    InvalidConfigError(String),
    #[error("error reconciling loadbalancer service: `{0}`")]
    LoadBalancerError(String),
    #[error("error querying Gateway API CRDs: `{0}`; are the CRDs installed?")]
    CRDNotFoundError(#[source] kube::Error),
    #[error("dataplane error: {0}")]
    DataplaneError(String),
    #[error("missing resource namespace")]
    MissingResourceNamespace,
    #[error("missing resource name")]
    MissingResourceName,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Clone, Hash, Eq, PartialEq)]
pub struct NamespacedName {
    pub name: String,
    pub namespace: String,
}

impl Display for NamespacedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.namespace.as_str())?;
        f.write_str("/")?;
        f.write_str(self.name.as_str())
    }
}

impl Debug for NamespacedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

pub trait NamespaceName {
    fn namespace(&self) -> std::result::Result<&str, Error>;
    fn name(&self) -> std::result::Result<&str, Error>;
    fn namespaced_name(&self) -> std::result::Result<NamespacedName, Error>;
}

impl NamespaceName for ObjectMeta {
    fn namespace(&self) -> std::result::Result<&str, Error> {
        self.namespace
            .as_deref()
            .ok_or(Error::MissingResourceNamespace)
    }

    fn name(&self) -> std::result::Result<&str, Error> {
        self.name.as_deref().ok_or(Error::MissingResourceName)
    }

    fn namespaced_name(&self) -> std::result::Result<NamespacedName, Error> {
        Ok(NamespacedName {
            name: self.name()?.to_string(),
            namespace: self.namespace()?.to_string(),
        })
    }
}
