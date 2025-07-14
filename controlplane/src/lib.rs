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
pub mod consts;
pub mod controllers;
mod gateway_utils;
mod gatewayclass_utils;
mod route_utils;
mod traits;

use kube::Client;
use thiserror::Error;

use crate::controllers::tcproute::TCPRouteError;

#[derive(Error, Debug)]
pub enum Error {
    #[error("kube error: {0}")]
    KubeError(#[source] kube::Error),
    #[error("invalid configuration: `{0}`")]
    InvalidConfigError(String),
    #[error("error reconciling loadbalancer service: `{0}`")]
    LoadBalancerError(String),
    #[error("error querying Gateway API CRDs: `{0}`; are the CRDs installed?")]
    CRDNotFoundError(#[source] kube::Error),
    #[error("dataplane error: {0}")]
    DataplaneError(String),
    #[error("error reconciling TCPRoute: {0}")]
    TCPRouteError(#[from] TCPRouteError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct NamespacedName {
    pub name: String,
    pub namespace: String,
}
