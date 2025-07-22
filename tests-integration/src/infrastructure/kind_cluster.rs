/*
Copyright 2025 The Kubernetes Authors.

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

use std::time::Duration;

use kube::config::KubeConfigOptions;
use kube::{Client, Config};
use thiserror::Error as ThisError;
use tracing::{error, info};

use crate::Result;
use crate::infrastructure::{
    AsyncCommand, AsyncCommandError, ContainerState, Workload, WorkloadImageTag,
};

/// Single-node kind cluster.
#[derive(Clone, Debug)]
pub struct KindCluster {
    name: String,
}

/// Errors originating from [`KindCluster`].
#[allow(missing_docs)]
#[derive(ThisError, Debug)]
pub enum KindClusterError {
    #[error("{0}: {1}")]
    Execution(String, AsyncCommandError),
    #[error("{0} container state {1:?}")]
    ContainerState(String, ContainerState),
    #[error("Failed to create client {1} for k8s context {0:?}")]
    Client(String, String),
    #[error("Could not determine container id for ter  {0}")]
    NotFound(String),
    #[error("loading image: {1}:{2} to cluster {0} failed: {3}")]
    LoadImage(String, String, String, String),
}

impl KindCluster {
    /// create a new cluster
    pub fn new<T: AsRef<str>>(name: T) -> Result<Self> {
        Ok(KindCluster {
            name: name.as_ref().to_string(),
        })
    }

    /// get the clusters name
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// get the k8s context name
    pub fn k8s_context(&self) -> String {
        format!("kind-{}", self.name)
    }

    /// get a `kube::Client` for the cluster
    pub async fn k8s_client(&self) -> Result<Client> {
        let kube_config = KubeConfigOptions {
            context: Some(self.k8s_context()),
            cluster: None,
            user: None,
        };
        let cfg = Config::from_kubeconfig(&kube_config)
            .await
            .map_err(|e| KindClusterError::Client(self.k8s_context(), e.to_string()))?;

        let client = Client::try_from(cfg)
            .map_err(|e| KindClusterError::Client(self.k8s_context(), e.to_string()))?;

        Ok(client)
    }

    /// create the cluster
    pub async fn create(&self) -> Result<()> {
        AsyncCommand::new("kind", &["create", "cluster", "--name", self.name.as_str()])
            .run()
            .await
            .map_err(|e| {
                KindClusterError::Execution(
                    format!("Failed to create kind cluster {}", self.name),
                    e,
                )
                .into()
            })
    }

    /// delete the cluster
    pub async fn delete(&self) -> Result<()> {
        AsyncCommand::new("kind", &["delete", "cluster", "--name", self.name.as_str()])
            .run()
            .await
            .map_err(|e| {
                KindClusterError::Execution(
                    format!("Failed to create kind cluster {}", self.name),
                    e,
                )
                .into()
            })
    }

    /// load a container image into the cluster
    pub async fn load_image(&self, image: &str, tag: &str) -> Result<()> {
        let kind_cluster = &self.name;
        info!("Loading image {image} with {tag} to kind cluster {kind_cluster:?}.");
        AsyncCommand::new(
            "kind",
            &[
                "load",
                "docker-image",
                format!("{image}:{tag}").as_str(),
                "--name",
                kind_cluster,
            ],
        )
        .run()
        .await
        .map_err(|e| {
            KindClusterError::LoadImage(
                kind_cluster.to_string(),
                image.to_string(),
                tag.to_string(),
                e.to_string(),
            )
            .into()
        })
    }

    /// In case wait_status is `None` the rollout will not wait for success
    /// In case wait_status is `Some(Duration)` the rollout waits for success with
    /// the duration as timeout in seconds
    pub async fn rollout<T: AsRef<WorkloadImageTag>>(
        &self,
        workload: T,
        wait_status: Option<Duration>,
    ) -> Result<()> {
        let workload = workload.as_ref();
        let k8s_ctx = self.k8s_context();

        let (workload_type, namespace, name) = workload.workload_namespace_name();

        // update deployment image references in case specified
        if let Some((image, tag)) = workload.image_tag() {
            info!(
                "Updating image {image} with tag {tag} for rollout {}.",
                workload.id
            );
            AsyncCommand::new(
                "kubectl",
                &[
                    format!("--context={k8s_ctx}").as_str(),
                    "set",
                    "image",
                    "-n",
                    namespace,
                    format!("{workload_type}/{name}").as_str(),
                    format!("*={image}:{tag}").as_str(),
                ],
            )
            .run()
            .await
            .map_err(|e| {
                KindClusterError::Execution(
                    format!("Failed to set image for {namespace} {workload_type}/{name} failed"),
                    e,
                )
            })?;
        }

        info!("Restarting rollout {}.", workload.id);
        AsyncCommand::new(
            "kubectl",
            &[
                format!("--context={k8s_ctx}").as_str(),
                "rollout",
                "restart",
                "-n",
                namespace,
                format!("{workload_type}/{name}").as_str(),
            ],
        )
        .run()
        .await
        .map_err(|e| {
            KindClusterError::Execution(
                format!("Rollout restart for {namespace} {workload_type}/{name} failed"),
                e,
            )
        })?;

        if let Some(wait_status) = wait_status {
            self.rollout_status(&workload.id, wait_status).await?;
        };

        Ok(())
    }

    /// In case wait_status is `None` the rollouts will not wait for success
    /// In case wait_status is `Some(Duration)` the rollouts are waiting for success with
    /// the duration as timeout in seconds per rollout
    /// the total timeout can reach up to the number of rollouts * wait_status `Duration`
    pub async fn rollouts<T: AsRef<WorkloadImageTag>>(
        &self,
        workloads: &[T],
        wait_status: Option<Duration>,
    ) -> Result<()> {
        for workload in workloads {
            self.rollout(workload, wait_status).await?;
        }
        Ok(())
    }

    /// wait for a rollout status to be successful
    pub async fn rollout_status<T: AsRef<Workload>>(
        &self,
        workload: T,
        timeout_secs: Duration,
    ) -> Result<()> {
        let k8s_ctx = self.k8s_context();
        let timeout = timeout_secs.as_secs().to_string();
        let workload = workload.as_ref();

        info!(
            "Waiting for rollout {} to complete. Timeout: {}s",
            workload, timeout
        );
        let (workload_type, namespace, name) = workload.workload_namespace_name();
        AsyncCommand::new(
            "kubectl",
            &[
                format!("--context={k8s_ctx}").as_str(),
                "rollout",
                "status",
                "-n",
                namespace,
                format!("{workload_type}/{name}").as_str(),
                "--timeout",
                format!("{timeout}s").as_str(),
            ],
        )
        .run()
        .await
        .map_err(|e| {
            KindClusterError::Execution(
                format!("Rollout status for {namespace} {workload_type}/{name} failed"),
                e,
            )
            .into()
        })
    }
}
