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

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tracing::{error, info};

use crate::Result;
use crate::infrastructure::{
    AsyncCommand, AsyncCommandError, ContainerRuntime, ImageTag, KindCluster, Workload,
    WorkloadImageTag, container_host_env,
};

/// Configuration and specification for container image builds.
pub struct ContainerImages {
    /// the directory provided is mounted in the container at /workspace_mount
    pub cargo_workspace_dir: String,
    /// the container runtime
    pub container_runtime: ContainerRuntime,
    /// the container host defaults to the environment variable `CONTAINER_HOST` or `unix:///run/podman/podman.sock`
    pub container_host: Option<String>,
    /// list of containers to process
    pub containers: Vec<Container>,
    /// the action to be taken during calling the `ContainerImages::process()` method
    pub action: ContainerImageAction,
    /// the image tag used for all containers, individual tags per container are not supported
    pub tag: String,
    /// used to build the fully qualified image name, omitted in case empty
    pub registry: Option<String>,
    /// the cluster used for related actions (e.g. `ImageAction::Load`)
    pub kind_cluster: Option<KindCluster>,
    /// set of commands that are executed serially before the image build starts
    pub pre_build_commands: Vec<AsyncCommand>,
}

/// Errors originating from [`ContainerImages`].
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum ContainerImageError {
    /// config contains an error, like requesting to load images without providing a cluster
    /// or a provided container file is not a valid/accessible path
    #[error("config is invalid: {0}")]
    InvalidConfig(String),
    #[error("pre build command failed: {0:?}")]
    PreBuild(AsyncCommandError),
    #[error("building image {1}:{2} with containerfile {0} failed: {3:?}")]
    Build(PathBuf, String, String, AsyncCommandError),
    #[error("loading image: {1}:{2} to cluster {0} failed: {3}")]
    Load(String, String, String, String),
}

/// Action that is taken when calling the [`ContainerImages::process()`] method.
#[allow(missing_docs)]
#[derive(Clone, Debug, Default)]
pub enum ContainerImageAction {
    #[default]
    Build,
    Load,
    Rollout,
    RolloutStatus,
}

/// Container details.
#[derive(Debug)]
pub struct Container {
    /// the path to the container file
    pub containerfile: PathBuf,
    /// the fully qualified image name without tag
    pub image_name: String,
    /// optional workload
    ///
    /// (e.g. when images are not correlated to a workload like having a k8s deployment that
    /// contains multiple containers and building all images)
    pub workload: Option<Workload>,
}

impl ContainerImages {
    /// execute the provided `ImageAction`
    /// validates the provided struct before processing
    ///
    /// the struct is built around public fields to avoid a `new` method
    /// that has a huge amount of inputs
    pub async fn process(&mut self) -> Result<()> {
        self.verify_config()?;

        for cmd in &mut self.pre_build_commands {
            cmd.run().await.map_err(ContainerImageError::PreBuild)?;
        }

        for container in self.containers.iter() {
            let image_tag = self.image_tag(container);

            if matches!(self.action, ContainerImageAction::Build)
                || matches!(self.action, ContainerImageAction::Load)
                || matches!(self.action, ContainerImageAction::Rollout)
            {
                self.build_image(&image_tag.image, &container.containerfile)
                    .await?
            };

            let Some(cluster) = &self.kind_cluster else {
                continue;
            };

            if matches!(self.action, ContainerImageAction::Load)
                || matches!(self.action, ContainerImageAction::Rollout)
            {
                cluster.load_image(&image_tag.image, &image_tag.tag).await?
            };

            if matches!(self.action, ContainerImageAction::Rollout)
                || matches!(self.action, ContainerImageAction::RolloutStatus)
            {
                if let Some(workload_id) = &container.workload {
                    let workload = WorkloadImageTag {
                        image_tag: Some(image_tag),
                        id: workload_id.clone(),
                    };
                    if matches!(self.action, ContainerImageAction::Rollout) {
                        cluster.rollout(&workload, None).await?;
                    };
                    if matches!(self.action, ContainerImageAction::RolloutStatus) {
                        cluster
                            .rollout(&workload, Some(Duration::from_secs(60)))
                            .await?;
                    };
                } else {
                    // no workload defined, no action required
                    continue;
                }
            }
        }

        Ok(())
    }

    fn verify_config(&self) -> Result<()> {
        match self.action {
            ContainerImageAction::Build => Ok(()),
            ContainerImageAction::Load
            | ContainerImageAction::Rollout
            | ContainerImageAction::RolloutStatus => {
                if self.kind_cluster.is_some() {
                    Ok(())
                } else {
                    Err(ContainerImageError::InvalidConfig(format!(
                        "Missing Kind cluster. Required for {:?}",
                        self.action
                    ))
                    .into())
                }
            }
        }
    }

    async fn build_image(&self, image: &str, containerfile: &PathBuf) -> Result<()> {
        let Some(containerfile_str) = containerfile.as_os_str().to_str() else {
            return Err(ContainerImageError::InvalidConfig(format!(
                "path invalid: {containerfile:?}"
            ))
            .into());
        };

        let build_timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(n) => n.as_millis(),
            Err(_) => 0,
        };

        info!("building image {image:?} using containerfile {containerfile:?}");
        AsyncCommand::new(
            self.container_runtime.to_string().to_lowercase().as_str(),
            &[
                "build",
                format!("--build-arg=BUILD_TIMESTAMP={build_timestamp}").as_str(),
                format!("-v={}:/workspace_mount/", self.cargo_workspace_dir).as_str(),
                "--file",
                containerfile_str,
                "-t",
                format!("{image}:{}", self.tag).as_str(),
                "./",
            ],
        )
        .working_dir(&self.cargo_workspace_dir)
        .env(container_host_env(self.container_host.as_ref()))
        .run()
        .await
        .map_err(|e| {
            ContainerImageError::Build(
                containerfile.clone(),
                image.to_string(),
                self.tag.to_string(),
                e,
            )
        })?;

        Ok(())
    }

    fn image_tag(&self, container: &Container) -> ImageTag {
        let image = if let Some(registry) = &self.registry {
            format!("{registry}/{}", container.image_name)
        } else {
            container.image_name.clone()
        };

        ImageTag {
            image,
            tag: self.tag.to_string(),
        }
    }

    /// get the workloads corresponding to the built images
    /// e.g. to restart workloads after loading the images to another cluster
    pub fn get_workloads(&self) -> Vec<WorkloadImageTag> {
        self.containers
            .iter()
            .filter(|c| c.workload.is_some())
            .map(|c| {
                let image_tag = self.image_tag(c);
                WorkloadImageTag {
                    image_tag: Some(image_tag),
                    id: c.workload.clone().unwrap(),
                }
            })
            .collect()
    }

    /// load the images to the `KindCluster`
    /// e.g. after building the cluster and images in parallel
    /// or loading the built images to another cluster
    pub async fn load_images(&self, cluster: &KindCluster) -> Result<()> {
        for container in self.containers.iter() {
            let image_tag = self.image_tag(container);
            cluster.load_image(&image_tag.image, &image_tag.tag).await?;
        }
        Ok(())
    }
}
