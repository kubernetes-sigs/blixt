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

pub mod consts;
pub mod controllers;
pub mod dataplane;
mod utils;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Client;
use thiserror::Error;

use crate::controllers::{GatewayError, TCPRouteError};
use crate::dataplane::DataplaneError;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    K8s(#[from] K8sError),
    #[error(transparent)]
    Dataplane(#[from] DataplaneError),
    #[error(transparent)]
    TCPRoute(#[from] TCPRouteError),
    #[error(transparent)]
    Gateway(#[from] GatewayError),
}

#[derive(Error, Debug)]
pub enum K8sError {
    #[error("kube client error: {0}")]
    Client(#[from] kube::Error),
    #[error("{0} missing property {1}")]
    MissingResourceProperty(String, String),
    #[error("{0} missing property {1}")]
    EmptyResourceProperty(String, String),
    #[error("missing resource namespace")]
    MissingResourceNamespace,
    #[error("missing resource name")]
    MissingResourceName,
}

impl K8sError {
    pub(crate) fn missing_resource_property(id: &NamespacedName, property: &str) -> K8sError {
        K8sError::MissingResourceProperty(
            format!("{}/{}", id.namespace, id.name),
            property.to_string(),
        )
    }
    pub(crate) fn empty_resource_property(id: &NamespacedName, property: &str) -> K8sError {
        K8sError::MissingResourceProperty(
            format!("{}/{}", id.namespace, id.name),
            property.to_string(),
        )
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct NamespacedName {
    pub name: String,
    pub namespace: String,
}

impl NamespacedName {
    pub(crate) fn new(namespace: &str, name: &str) -> Self {
        Self {
            name: name.to_string(),
            namespace: namespace.to_string(),
        }
    }
}

pub(crate) trait NamespaceName {
    fn namespace(&self) -> Result<&str>;
    fn name(&self) -> Result<&str>;
    fn namespaced_name(&self) -> Result<NamespacedName>;
}

impl NamespaceName for ObjectMeta {
    fn namespace(&self) -> Result<&str> {
        self.namespace
            .as_deref()
            .ok_or(K8sError::MissingResourceNamespace.into())
    }

    fn name(&self) -> Result<&str> {
        self.name
            .as_deref()
            .ok_or(K8sError::MissingResourceName.into())
    }

    fn namespaced_name(&self) -> Result<NamespacedName> {
        Ok(NamespacedName {
            name: self.name()?.to_string(),
            namespace: self.namespace()?.to_string(),
        })
    }
}
