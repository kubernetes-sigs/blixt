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
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client};
use thiserror::Error as ThisError;
use tokio::sync::RwLock;
use tonic::Request;
use tonic::transport::Channel;
use tracing::{debug, info, warn};

use crate::{Error, K8sError, NamespaceName, NamespacedName};

#[derive(Clone)]
pub struct DataplaneClientManager {
    clients: Arc<RwLock<HashMap<NamespacedName, BackendsClient<Channel>>>>,
    service_port: u16,
    service_app_label: String,
    service_component_label: String,
    service_namespace: String,
}

#[derive(ThisError, Debug)]
pub enum DataplaneError {
    #[error("no dataplane clients available")]
    MissingClients,
    #[error("failed to connect to dataplane pod {0} error {1}")]
    PodConnectionFailed(NamespacedName, Box<tonic::transport::Error>),
    #[error("Failed to update targets on dataplane pod {0} status {1}")]
    UpdateFailed(NamespacedName, Box<tonic::Status>),
}

impl DataplaneClientManager {
    pub fn new(
        service_namespace: String,
        service_app_label: String,
        service_component_label: String,
        service_port: u16,
    ) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            service_port,
            service_app_label,
            service_component_label,
            service_namespace,
        }
    }

    pub async fn update_clients(&self, client: Client) -> Result<(), Error> {
        let pod_api: Api<Pod> = Api::namespaced(client, &self.service_namespace);

        let dataplane_pods = pod_api
            .list(&Default::default())
            .await
            .map_err(K8sError::client)?
            .items
            .into_iter()
            .filter(|pod| match pod.metadata.labels.as_ref() {
                Some(labels) => {
                    labels.get("app") == Some(&self.service_app_label)
                        && labels.get("component") == Some(&self.service_component_label)
                }
                None => false,
            })
            .collect::<Vec<Pod>>();

        let mut new_clients = HashMap::new();

        for pod in dataplane_pods {
            let pod_id = pod.metadata.namespaced_name()?;
            if let Some(pod_ip) = &pod.status.as_ref().and_then(|s| s.pod_ip.as_ref()) {
                let endpoint = format!("http://{pod_ip}:{}", self.service_port);
                match BackendsClient::connect(endpoint.clone()).await {
                    Ok(grpc_client) => {
                        info!("Connected to dataplane pod {pod_id} on endpoint {endpoint}");
                        new_clients.insert(pod_id, grpc_client);
                    }
                    Err(err) => {
                        return Err(DataplaneError::PodConnectionFailed(pod_id, err.into()).into());
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

        for (pod_id, mut client) in clients.clone() {
            match client.update(Request::new(targets.clone())).await {
                Ok(resp) => {
                    info!("Successfully updated targets on dataplane pod: {pod_id}");
                    debug!("Received {:?}", resp.get_ref());
                }
                Err(err) => {
                    return Err(DataplaneError::UpdateFailed(pod_id, err.into()).into());
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

        for (pod_id, mut client) in clients.clone() {
            match client.delete(Request::new(vip)).await {
                Ok(resp) => {
                    info!("Successfully deleted VIP on dataplane pod: {pod_id}");
                    debug!("Received {:?}", resp.get_ref());
                }
                Err(err) => {
                    warn!("Failed to delete VIP on dataplane pod {pod_id}: {err:?}");
                }
            }
        }

        Ok(())
    }
}
