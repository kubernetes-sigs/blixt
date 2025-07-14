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
mod gateway_utils;
mod route_utils;
mod traits;

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
    #[error("{0}/{1} missing property {2}")]
    MissingResourceProperty(String, String, String),
    #[error("missing resource namespace")]
    MissingResourceNamespace,
    #[error("missing resource name")]
    MissingResourceName,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Clone, Debug)]
pub struct NamespacedName {
    pub name: String,
    pub namespace: String,
}
