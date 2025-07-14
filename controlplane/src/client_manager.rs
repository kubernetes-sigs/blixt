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

use std::collections::HashMap;
use std::sync::Arc;

use api_server::backends::{Targets, Vip, backends_client::BackendsClient};
use gateway_api::apis::standard::gateways::Gateway;
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client};
use thiserror::Error as ThisError;
use tokio::sync::RwLock;
use tonic::Request;
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::consts::{BLIXT_APP_LABEL, BLIXT_DATAPLANE_COMPONENT_LABEL, BLIXT_NAMESPACE};
use crate::controllers::{GatewayError, NamespaceName};
use crate::{Error, K8sError};

#[derive(Clone)]
pub struct DataplaneClientManager {
    clients: Arc<RwLock<HashMap<String, BackendsClient<Channel>>>>,
}

#[derive(ThisError, Debug)]
pub enum DataplaneError {
    #[error("no dataplane clients available")]
    MissingClients,
    #[error("failed to connect to dataplane pod {0} error {1}")]
    PodConnectionFailed(String, Box<tonic::transport::Error>),
    #[error("Failed to update targets on dataplane pod {0} status {1}")]
    UpdateFailed(String, Box<tonic::Status>),
}

impl Default for DataplaneClientManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DataplaneClientManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn update_clients(&self, client: Client) -> Result<(), Error> {
        let pod_api: Api<Pod> = Api::namespaced(client, BLIXT_NAMESPACE);

        let dataplane_pods = pod_api
            .list(&Default::default())
            .await
            .map_err(K8sError::Client)?
            .items
            .into_iter()
            .filter(|pod| match pod.metadata.labels.as_ref() {
                Some(labels) => {
                    labels.get("app") == Some(&BLIXT_APP_LABEL.to_string())
                        && labels.get("component")
                            == Some(&BLIXT_DATAPLANE_COMPONENT_LABEL.to_string())
                }
                None => false,
            })
            .collect::<Vec<Pod>>();

        let mut new_clients = HashMap::new();

        for pod in dataplane_pods {
            if let Some(pod_ip) = &pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) {
                // FIXME: allow to configure port via CLI arg
                let endpoint = format!("http://{pod_ip}:9874");
                match BackendsClient::connect(endpoint.clone()).await {
                    Ok(grpc_client) => {
                        info!("Connected to dataplane pod: {}", pod_ip);
                        new_clients.insert(pod_ip.to_string(), grpc_client);
                    }
                    Err(err) => {
                        return Err(DataplaneError::PodConnectionFailed(
                            pod_ip.as_str().to_string(),
                            err.into(),
                        )
                        .into());
                    }
                }
            }
        }

        let mut clients = self.clients.write().await;
        *clients = new_clients;

        Ok(())
    }

    pub async fn update_targets(&self, targets: Targets) -> Result<(), Error> {
        let clients = self.clients.read().await;
        if clients.is_empty() {
            return Err(DataplaneError::MissingClients.into());
        }

        for (pod_ip, mut client) in clients.clone() {
            match client.update(Request::new(targets.clone())).await {
                Ok(resp) => {
                    info!("Successfully updated targets on dataplane pod: {}", pod_ip);
                    info!("Received {:?}", resp.get_ref());
                }
                Err(err) => {
                    return Err(DataplaneError::UpdateFailed(pod_ip, err.into()).into());
                }
            }
        }

        Ok(())
    }

    pub async fn delete_vip(&self, vip: Vip) -> Result<(), Error> {
        let clients = self.clients.read().await;
        if clients.is_empty() {
            return Err(DataplaneError::MissingClients.into());
        }

        for (pod_ip, mut client) in clients.clone() {
            match client.delete(Request::new(vip)).await {
                Ok(_) => {
                    info!("Successfully deleted VIP on dataplane pod: {}", pod_ip);
                }
                Err(err) => {
                    warn!("Failed to delete VIP on dataplane pod {}: {}", pod_ip, err);
                }
            }
        }

        Ok(())
    }
}

pub fn get_gateway_ip(gateway: &Gateway) -> Result<std::net::Ipv4Addr, Error> {
    let gw_name = gateway.metadata.name()?;
    let namespace = gateway.metadata.namespace()?;
    gateway
        .status
        .as_ref()
        .and_then(|status| status.addresses.as_ref())
        .and_then(|addresses| addresses.first())
        .and_then(|addr| addr.value.parse().ok())
        .ok_or_else(|| GatewayError::IpNotFound(namespace, gw_name).into())
}
