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

use std::collections::BTreeMap;
use std::ops::Add;
use std::process::Stdio;
use std::time::{Duration, Instant};

use chrono::Utc;
use k8s_openapi::api::apps::v1::{
    DaemonSet, DaemonSetSpec, Deployment, DeploymentSpec, ReplicaSet,
};
use k8s_openapi::api::core::v1::{Container, Pod, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::chrono;
use kube::api::{ListParams, LogParams, Patch, PatchParams};
use kube::config::KubeConfigOptions;
use kube::core::Selector;
use kube::{Api, Client, Config};
use thiserror::Error as ThisError;
use tokio::time::sleep;
use tracing::{error, info};

use crate::infrastructure::{AsyncCommand, AsyncCommandError, Workload, WorkloadImageTag};

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
    #[error("{0}")]
    Rollout(String),
    #[error("kube client error: {0}")]
    Client(#[from] Box<kube::Error>),
    #[error("Failed to create client {1} for k8s context {0:?}")]
    Config(String, String),
    #[error("loading image: {1}:{2} to cluster {0} failed: {3}")]
    LoadImage(String, String, String, String),
}

impl From<kube::Error> for KindClusterError {
    fn from(value: kube::Error) -> Self {
        Box::new(value).into()
    }
}

pub type Result<T, E = KindClusterError> = std::result::Result<T, E>;

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
            .map_err(|e| KindClusterError::Config(self.k8s_context(), e.to_string()))?;

        let client = Client::try_from(cfg)
            .map_err(|e| KindClusterError::Config(self.k8s_context(), e.to_string()))?;

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
            })

        self.host_mount_bpf_fs().await
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
            })
    }

    pub async fn host_mount_bpf_fs(&self) -> Result<()> {
        let container_name = format!("{}-control-plane", self.name);
        AsyncCommand::new(
            "docker",
            &[
                "exec",
                container_name.as_str(),
                "/bin/sh",
                "-c",
                "mount -t bpf -o shared,rw,nosuid,nodev,noexec,realtime,mode=700 bpffs /sys/fs/bpf",
            ],
        )
        .run()
        .await
        .map_err(|e| {
            KindClusterError::Execution(
                format!("Failed to mount bpf fs on host container {container_name}"),
                e,
            )
            .into()
        })
    }

    /// load a container image into the cluster
    pub async fn load_image(&self, image: &str, tag: &str) -> Result<()> {
        let kind_cluster = &self.name;
        info!("Loading image {image} with {tag} to kind cluster {kind_cluster:?}.");
        let mut image_save = AsyncCommand::new(
            "podman",
            &[
                "image",
                "save",
                format!("{image}:{tag}").as_str(),
                "-o",
                "/dev/stdout",
            ],
        );
        let mut kind_load = AsyncCommand::new(
            "kind",
            &[
                "--name",
                kind_cluster,
                "load",
                "image-archive",
                "/dev/stdin",
            ],
        );

        image_save.cmd.stdout(Stdio::piped());
        let image_save = image_save.cmd.spawn().map_err(|e| {
            KindClusterError::LoadImage(
                kind_cluster.to_string(),
                image.to_string(),
                tag.to_string(),
                AsyncCommandError::Spawn(e).to_string(),
            )
        })?;

        let Some(stdout) = image_save.stdout else {
            return Err(KindClusterError::LoadImage(
                kind_cluster.to_string(),
                image.to_string(),
                tag.to_string(),
                "Failed to get stdout from image_save process.".to_string(),
            )
            .into());
        };
        let fd = stdout.into_owned_fd().map_err(|e| {
            KindClusterError::LoadImage(
                kind_cluster.to_string(),
                image.to_string(),
                tag.to_string(),
                AsyncCommandError::Output(e).to_string(),
            )
        })?;

        kind_load.cmd.stdin(std::process::ChildStdout::from(fd));
        kind_load.run().await.map_err(|e| {
            KindClusterError::LoadImage(
                kind_cluster.to_string(),
                image.to_string(),
                tag.to_string(),
                e.to_string(),
            )
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
        let client = self.k8s_client().await?;
        let pp = PatchParams::apply("blixt-integration-tests");

        let workload = workload.as_ref();
        let (namespace, name) = workload.id.namespace_name();

        match &workload.id {
            Workload::DaemonSet(_) => {
                let daemonset_api = Api::<DaemonSet>::namespaced(client.clone(), namespace);
                let daemonset = daemonset_api.get(name).await?;

                let Some(spec) = daemonset.spec.unwrap_or_default().template.spec else {
                    return Err(KindClusterError::Rollout(format!(
                        "{} does not contain .spec.template.spec",
                        workload.id
                    )));
                };

                let patch = DaemonSet {
                    metadata: ObjectMeta {
                        name: Some(name.to_string()),
                        ..Default::default()
                    },
                    spec: Some(DaemonSetSpec {
                        template: Self::container_image_update_rollout_patch(spec, workload),
                        ..Default::default()
                    }),
                    ..Default::default()
                };

                let patch = Patch::Strategic(&patch);
                daemonset_api.patch(name, &pp, &patch).await?;
            }
            Workload::Deployment(_) => {
                let deployment_api = Api::<Deployment>::namespaced(client.clone(), namespace);
                let deployment = deployment_api.get(name).await?;

                let Some(spec) = deployment.spec.unwrap_or_default().template.spec else {
                    return Err(KindClusterError::Rollout(format!(
                        "{} does not contain .spec.template.spec",
                        workload.id
                    )));
                };

                let patch = Deployment {
                    metadata: ObjectMeta {
                        name: Some(name.to_string()),
                        ..Default::default()
                    },
                    spec: Some(DeploymentSpec {
                        template: Self::container_image_update_rollout_patch(spec, workload),
                        ..Default::default()
                    }),
                    ..Default::default()
                };

                let patch = Patch::Strategic(&patch);
                deployment_api.patch(name, &pp, &patch).await?;
            }
        }

        if let Some(wait_status) = wait_status {
            self.rollout_status(&workload.id, wait_status).await?;
        };

        Ok(())
    }

    fn container_image_update_rollout_patch(
        spec: PodSpec,
        workload: &WorkloadImageTag,
    ) -> PodTemplateSpec {
        let mut container_patches = vec![];

        // update deployment image references in case specified
        if let Some(image_tag) = &workload.image_tag {
            for container in spec.containers {
                if let Some(container_image) = &container.image {
                    let update_image = format!("{}:{}", image_tag.image, image_tag.tag);
                    if container_image != &update_image {
                        container_patches.push(Container {
                            image: Some(update_image),
                            name: container.name.clone(),
                            ..Default::default()
                        })
                    };
                }
            }
        }

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "blixt.integration.tests/restartedAt".to_string(),
            Utc::now().to_rfc3339(),
        );

        if container_patches.is_empty() {
            info!("Requesting rollout for {}.", workload.id);
            // only update annotations to ensure rollout is triggered
            PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    annotations: Some(annotations),
                    ..Default::default()
                }),
                ..Default::default()
            }
        } else {
            if let Some(image_tag) = &workload.image_tag {
                info!(
                    "Updating image for {} to {}:{}",
                    workload.id, image_tag.image, image_tag.tag
                );
            }
            PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    annotations: Some(annotations),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: container_patches,
                    ..Default::default()
                }),
            }
        }
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
        let client = self.k8s_client().await?;
        let workload = workload.as_ref();
        let (namespace, name) = workload.namespace_name();

        let start_time = Instant::now();
        'watch: while start_time.elapsed() <= timeout_secs.add(Duration::from_secs(1)) {
            // wait first to avoid potentially getting old rollout details
            sleep(Duration::from_secs(1)).await;

            let rollout_success = match workload {
                Workload::DaemonSet(_) => {
                    Self::rollout_status_daemonset(
                        client.clone(),
                        namespace,
                        name,
                        &start_time,
                        &timeout_secs,
                    )
                    .await?
                }
                Workload::Deployment(_) => {
                    Self::rollout_status_deployment(
                        client.clone(),
                        namespace,
                        name,
                        &start_time,
                        &timeout_secs,
                    )
                    .await?
                }
            };

            if rollout_success {
                info!("Rollout for {workload} was successful.");
                break 'watch;
            } else {
                info!(
                    "Waiting for {workload} rollout to complete (elapsed: {:?}s, timeout: {timeout_secs:?}).",
                    start_time.elapsed().as_secs()
                );
            }
        }

        Ok(())
    }

    async fn rollout_status_deployment(
        client: Client,
        namespace: &str,
        name: &str,
        start_time: &Instant,
        timeout: &Duration,
    ) -> Result<bool> {
        let deployment_api = Api::<Deployment>::namespaced(client, namespace);
        let deployment = deployment_api.get(name).await?;
        let Some(status) = deployment.status.clone() else {
            return Ok(false);
        };
        let Some(deployment_revision) = deployment
            .metadata
            .annotations
            .clone()
            .unwrap_or_default()
            .remove("deployment.kubernetes.io/revision")
        else {
            return Ok(false);
        };

        // locate corresponding ReplicaSet
        let lp = if let Some(labels) = deployment.metadata.labels {
            ListParams::default().labels_from(&Selector::from_iter(labels))
        } else {
            ListParams::default()
        };

        let replicaset_api = Api::<ReplicaSet>::namespaced(deployment_api.into_client(), namespace);
        let replicasets = replicaset_api.list(&lp).await?;

        let Some(replicaset) = replicasets.into_iter().find(|replicaset| {
            let replicaset_revision = replicaset
                .metadata
                .annotations
                .clone()
                .unwrap_or_default()
                .remove("deployment.kubernetes.io/revision")
                .unwrap_or_default();
            replicaset_revision == deployment_revision
        }) else {
            return Ok(false);
        };

        let pod_api = Api::<Pod>::namespaced(replicaset_api.into_client(), namespace);
        let replicaset_labels = replicaset.metadata.labels.clone().unwrap_or_default();
        // ReplicaSet labels contain pod-template-hash to identify corresponding pods
        let lp = ListParams::default().labels_from(&Selector::from_iter(replicaset_labels));
        let pods = pod_api.list(&lp).await?.items;

        if &start_time.elapsed() >= timeout {
            error!("Deployment {namespace}/{name} rollout timed out.",);
            error!("{:?}", status);
            error!("{:?}", replicaset.status);

            for pod in pods {
                Self::error_pod_details(&pod_api, pod).await?;
            }

            Err(KindClusterError::Rollout(format!(
                "Deployment {namespace}/{name} rollout timed out."
            )))
        } else if pods.is_empty() {
            Ok(false)
        } else {
            let pods_running = pods.iter().all(|p| {
                p.status
                    .clone()
                    .unwrap_or_default()
                    .phase
                    .unwrap_or_default()
                    == "Running"
            });
            let deployment_ready = status.ready_replicas >= status.replicas;
            Ok(deployment_ready && pods_running)
        }
    }

    async fn rollout_status_daemonset(
        client: Client,
        namespace: &str,
        name: &str,
        start_time: &Instant,
        timeout: &Duration,
    ) -> Result<bool> {
        let daemonset_api = Api::<DaemonSet>::namespaced(client, namespace);
        let daemonset = daemonset_api.get(name).await?;
        let Some(status) = daemonset.status.clone() else {
            return Ok(false);
        };

        let daemonset_generation = daemonset
            .metadata
            .generation
            .unwrap_or_default()
            .to_string();

        let lp = ListParams::default().labels_from(&Selector::from_iter(
            daemonset.metadata.labels.unwrap_or_default(),
        ));
        let pod_api = Api::<Pod>::namespaced(daemonset_api.into_client(), namespace);
        let pods = pod_api.list(&lp).await?;

        let pods = pods
            .items
            .into_iter()
            .filter(|p| {
                let mut labels = p.metadata.labels.clone().unwrap_or_default();
                let pod_template_generation =
                    labels.remove("pod-template-generation").unwrap_or_default();
                pod_template_generation == daemonset_generation
            })
            .collect::<Vec<Pod>>();

        if &start_time.elapsed() >= timeout {
            error!("DaemonSet {namespace}/{name} rollout timed out.",);
            error!("{:?}", status);

            for pod in pods {
                Self::error_pod_details(&pod_api, pod).await?;
            }

            Err(KindClusterError::Rollout(format!(
                "DaemonSet {namespace}/{name} rollout timed out."
            )))
        } else if pods.is_empty() {
            Ok(false)
        } else {
            let pods_running = pods.iter().all(|p| {
                p.status
                    .clone()
                    .unwrap_or_default()
                    .phase
                    .unwrap_or_default()
                    == "Running"
            });
            let daemonset_ready = status.number_ready >= status.desired_number_scheduled;
            Ok(daemonset_ready && pods_running)
        }
    }

    /// log pod status and pod logs
    async fn error_pod_details(pod_api: &Api<Pod>, pod: Pod) -> Result<()> {
        if let Some(status) = pod.status {
            error!("{:?}", status);
        }
        if let Some(name) = pod.metadata.name {
            let lp = LogParams {
                tail_lines: Some(1024),
                ..Default::default()
            };

            let pod_logs = pod_api.logs(name.as_str(), &lp).await?;
            error!("{pod_logs}")
        }
        Ok(())
    }
}
