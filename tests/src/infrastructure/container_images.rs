use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tracing::info;
use xshell::{Shell, cmd};

use crate::Result;
use crate::infrastructure::{ContainerRuntime, ImageTag, KindCluster, Workload, WorkloadImageTag};

#[derive(Clone, Debug, Default)]
pub enum ImageAction {
    Build,
    #[default]
    Load,
    Rollout,
    RolloutWait,
}

pub struct ContainerImages {
    pub cargo_workspace_dir: String,
    pub container_runtime: ContainerRuntime,
    pub container_host: Option<String>,
    pub containers: Vec<Container>,
    pub action: ImageAction,
    pub tag: String,
    pub registry: Option<String>,
    pub kind_cluster: Option<KindCluster>,
}

#[derive(Debug)]
pub struct Container {
    pub containerfile: PathBuf,
    pub image_name: String,
    pub workload: Option<Workload>,
}

#[derive(Error, Debug)]
pub enum ImageError {
    #[error("Failed to create shell: {0}")]
    CouldNotCreateShell(xshell::Error),
    #[error("Failed to create shell: {0}")]
    CargoBuild(xshell::Error),
    #[error("Failed to build image {1}:{2} with containerfile {0}")]
    Build(PathBuf, String, String),
    #[error("Failed to load image: {0}:{1}")]
    Load(String, String),
    #[error("Failed to start image: {0}:{1}")]
    Start(String, String),
    #[error("config is invalid: {0}")]
    InvalidConfig(String),
}

impl ContainerImages {
    pub async fn process(&self) -> Result<()> {
        self.verify_config()?;

        let sh = match Shell::new() {
            Ok(sh) => sh,
            Err(e) => return Err(ImageError::CouldNotCreateShell(e).into()),
        };

        info!("running cargo build");
        cmd!(sh, "cargo build")
            .run()
            .map_err(ImageError::CargoBuild)?;

        let _env_guard = if let Some(container_host) = &self.container_host {
            sh.push_env("CONTAINER_HOST", container_host)
        } else {
            sh.push_env("CONTAINER_HOST", "unix:///run/podman/podman.sock")
        };

        for container in self.containers.iter() {
            let image = self.image_tag(container).image;

            if matches!(self.action, ImageAction::Build)
                || matches!(self.action, ImageAction::Load)
                || matches!(self.action, ImageAction::Rollout)
            {
                self.build_image(&sh, &image, &container.containerfile)?
            };

            let Some(cluster) = &self.kind_cluster else {
                continue;
            };

            if matches!(self.action, ImageAction::Load)
                || matches!(self.action, ImageAction::Rollout)
            {
                cluster.load_image(&image, &self.tag).await?
            };

            if matches!(self.action, ImageAction::Rollout)
                || matches!(self.action, ImageAction::RolloutWait)
            {
                if let Some(workload_id) = &container.workload {
                    let workload = WorkloadImageTag {
                        image_tag: Some(ImageTag {
                            image: image.to_string(),
                            tag: self.tag.to_string(),
                        }),
                        id: workload_id.clone(),
                    };
                    if matches!(self.action, ImageAction::Rollout) {
                        cluster.rollout(&workload, None).await?;
                    };
                    if matches!(self.action, ImageAction::RolloutWait) {
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
            ImageAction::Build => Ok(()),
            ImageAction::Load | ImageAction::Rollout | ImageAction::RolloutWait => {
                if self.kind_cluster.is_some() {
                    Ok(())
                } else {
                    Err(ImageError::InvalidConfig(format!(
                        "Missing Kind cluster. Required for {:?}",
                        self.action
                    ))
                    .into())
                }
            }
        }
    }

    fn build_image(&self, sh: &Shell, image: &str, containerfile: &PathBuf) -> Result<()> {
        let container_runtime = format!("{:?}", self.container_runtime).to_lowercase();
        let manifest_dir = &self.cargo_workspace_dir;
        let volumes = format!("-v={manifest_dir}/:/workspace/");
        let tag = &self.tag;

        info!("building image {image:?} using containerfile {containerfile:?}");
        cmd!(
            sh,
            "{container_runtime} build --no-cache {volumes} --file {containerfile} -t {image}:{tag}"
        )
        .run()
        .map_err(|_| {
            ImageError::Build(containerfile.clone(), image.to_string(), tag.to_string()).into()
        })
        .map(|_| ())
    }

    pub fn get_workloads(&self) -> Vec<WorkloadImageTag> {
        self.containers
            .iter()
            .filter(|c| c.workload.is_some())
            .map(|c| {
                let image_tag = self.image_tag(c);
                WorkloadImageTag {
                    image_tag: Some(ImageTag {
                        image: image_tag.image,
                        tag: image_tag.tag,
                    }),
                    id: c.workload.clone().unwrap(),
                }
            })
            .collect()
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
}
