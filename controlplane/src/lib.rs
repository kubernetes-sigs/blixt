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

use gateway_api::apis::experimental::gateways::Gateway;
use gateway_api::apis::experimental::tcproutes::TCPRoute;
use gateway_api::apis::experimental::udproutes::UDPRoute;
use gateway_api::gatewayclasses::GatewayClass;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::ListParams;
use kube::{Api, Client};
use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;
use tracing::{error, warn};

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
    Client(#[from] Box<kube::Error>),
    #[error("{0} missing property {1}")]
    MissingResourceProperty(String, String),
    #[error("{0} missing property {1}")]
    EmptyResourceProperty(String, String),
    #[error("missing resource namespace")]
    MissingResourceNamespace,
    #[error("missing resource name")]
    MissingResourceName,
    #[error("GatewayApi Custom Resource Definitions ({0}) are likely not installed.")]
    GatewayApiNotInstalled(String, Box<kube::Error>),
}

impl K8sError {
    pub(crate) fn missing_resource_property(id: &NamespacedName, property: &str) -> Self {
        K8sError::MissingResourceProperty(
            format!("{}/{}", id.namespace, id.name),
            property.to_string(),
        )
    }
    pub(crate) fn empty_resource_property(id: &NamespacedName, property: &str) -> Self {
        K8sError::MissingResourceProperty(
            format!("{}/{}", id.namespace, id.name),
            property.to_string(),
        )
    }
    pub(crate) fn client(err: kube::Error) -> Self {
        K8sError::Client(Box::new(err))
    }
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
    fn namespace(&self) -> std::result::Result<&str, K8sError>;
    fn name(&self) -> std::result::Result<&str, K8sError>;
    fn namespaced_name(&self) -> std::result::Result<NamespacedName, K8sError>;
}

impl NamespaceName for ObjectMeta {
    fn namespace(&self) -> std::result::Result<&str, K8sError> {
        self.namespace
            .as_deref()
            .ok_or(K8sError::MissingResourceNamespace)
    }

    fn name(&self) -> std::result::Result<&str, K8sError> {
        self.name.as_deref().ok_or(K8sError::MissingResourceName)
    }

    fn namespaced_name(&self) -> std::result::Result<NamespacedName, K8sError> {
        Ok(NamespacedName {
            name: self.name()?.to_string(),
            namespace: self.namespace()?.to_string(),
        })
    }
}

pub async fn check_gateway_api_installed(k8s_client: Client, namespace: &str) -> Result<()> {
    let gateway_class_api = Api::<GatewayClass>::all(k8s_client);
    gateway_class_api
        .list(&ListParams::default().limit(1))
        .await
        .map_err(|e| match e {
            kube::Error::Api(kube::core::ErrorResponse { code: 404, .. }) => {
                error!("Listing GatewayClass resources on k8s API error 404.");
                K8sError::GatewayApiNotInstalled("GatewayClass".to_string(), Box::new(e))
            }
            _ => K8sError::client(e),
        })?;

    let gateway_api = Api::<Gateway>::namespaced(gateway_class_api.into_client(), namespace);
    gateway_api
        .list(&ListParams::default().limit(1))
        .await
        .map_err(|e| match e {
            kube::Error::Api(kube::core::ErrorResponse { code: 404, .. }) => {
                error!("Listing Gateway resources on k8s API error 404.");
                K8sError::GatewayApiNotInstalled("Gateway".to_string(), Box::new(e))
            }
            _ => K8sError::client(e),
        })?;

    let tcp_route_api = Api::<TCPRoute>::all(gateway_api.into_client());
    tcp_route_api
        .list(&ListParams::default().limit(1))
        .await
        .map_err(|e| match e {
            kube::Error::Api(kube::core::ErrorResponse { code: 404, .. }) => {
                error!("Listing TCPRoute resources on k8s API error 404.");
                K8sError::GatewayApiNotInstalled("TCPRoute".to_string(), Box::new(e))
            }
            _ => K8sError::client(e),
        })?;

    let udp_route_api = Api::<UDPRoute>::all(tcp_route_api.into_client());
    udp_route_api
        .list(&ListParams::default().limit(1))
        .await
        .map_err(|e| match e {
            kube::Error::Api(kube::core::ErrorResponse { code: 404, .. }) => {
                warn!("Listing UDPRoute on k8s API error 404.");
                K8sError::GatewayApiNotInstalled("UDPRoute".to_string(), Box::new(e))
            }
            _ => K8sError::client(e),
        })?;

    Ok(())
}
