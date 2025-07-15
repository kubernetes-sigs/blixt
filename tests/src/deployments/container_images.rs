use std::path::PathBuf;
use thiserror::Error;
use tracing::info;
use xshell::{Shell, cmd};

use crate::Result;
use crate::deployments::{ContainerRuntime, KindCluster, KindError, KustomizeError, Workload};

#[derive(Clone, Debug, Default)]
pub enum ImageAction {
    Build,
    #[default]
    Load,
    Start,
}

pub struct Images {
    pub cargo_workspace_dir: String,
    pub container_runtime: ContainerRuntime,
    pub container_host: Option<String>,
    pub containers: Vec<Container>,
    pub action: ImageAction,
    pub tag: String,
    pub registry: Option<String>,
    pub kind_cluster: KindCluster,
}

struct ImageTag {
    image: String,
    tag: String,
}

pub struct WorkloadImageTag {
    pub image: String,
    pub tag: String,
    pub workload: Workload,
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
    CargoBuildFailed(xshell::Error),
    #[error("Failed to build image {1}:{2} with containerfile {0}")]
    ImageBuildFailed(PathBuf, String, String),
    #[error("Failed to load image: {0}:{1}")]
    ImageLoadFailed(String, String),
    #[error("Failed to start image: {0}:{1}")]
    ImageStartFailed(String, String),
}

// TODO: eventually extract KindCluster to isolate build
// and call load(kind: KindCluster), and start(kind: KindCluster)
impl Images {
    pub async fn process(&self) -> Result<()> {
        let sh = match Shell::new() {
            Ok(sh) => sh,
            Err(e) => return Err(ImageError::CouldNotCreateShell(e).into()),
        };

        info!("running cargo build");
        cmd!(sh, "cargo build")
            .run()
            .map_err(ImageError::CargoBuildFailed)?;

        let _env_guard = if let Some(container_host) = &self.container_host {
            sh.push_env("CONTAINER_HOST", container_host)
        } else {
            sh.push_env("CONTAINER_HOST", "unix:///run/podman/podman.sock")
        };

        for container in self.containers.iter() {
            let image = self.image_tag(container).image;

            if matches!(self.action, ImageAction::Build)
                || matches!(self.action, ImageAction::Load)
                || matches!(self.action, ImageAction::Start)
            {
                self.build_image(&sh, &image, &container.containerfile)?
            };

            if matches!(self.action, ImageAction::Load) || matches!(self.action, ImageAction::Start)
            {
                self.load_image(&sh, &image)?
            };

            if matches!(self.action, ImageAction::Start) {
                self.start_image(&sh, &image, &container.workload)?
            }
        }

        Ok(())
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
            ImageError::ImageBuildFailed(containerfile.clone(), image.to_string(), tag.to_string())
                .into()
        })
        .map(|_| ())
    }

    fn load_image(&self, sh: &Shell, image: &str) -> Result<()> {
        let tag = &self.tag;
        let kind_cluster = self.kind_cluster.name();

        info!("loading image {image:?} to kind cluster {kind_cluster:?}");
        cmd!(
            sh,
            "kind load docker-image {image}:{tag} --name {kind_cluster}"
        )
        .run()
        .map_err(|_| ImageError::ImageLoadFailed(image.to_string(), tag.to_string()).into())
        .map(|_| ())
    }

    fn start_image(&self, sh: &Shell, image: &str, workload: &Option<Workload>) -> Result<()> {
        info!("{image}");
        let image = *image.split(":").collect::<Vec<_>>().first().unwrap();
        let image = *image.split("/").collect::<Vec<_>>().last().unwrap();
        info!("{image}");

        let Some(workload) = workload else {
            return Ok(());
        };

        let (workload, namespace, name) = match workload {
            Workload::DaemonSet(d) => ("daemonset", &d.namespace, &d.name),
            Workload::Deployment(d) => ("deployment", &d.namespace, &d.name),
        };

        let k8s_ctx = self.kind_cluster.k8s_context();
        info!("restarting kubernetes {workload}/{name}");
        cmd!(
            sh,
            "kubectl --context={k8s_ctx} -n {namespace} rollout restart {workload}/{name}"
        )
        .run()
        .map_err(|_| ImageError::ImageStartFailed(workload.to_string(), name.to_string()).into())
        .map(|_| ())
    }

    // currently sync, provisional async in case cmd is replaced
    pub async fn rollout_restart(&self, wait_status: bool) -> Result<()> {
        let workload_image_tags = self.workload_image_tags();
        let sh = Shell::new().map_err(|e| KustomizeError::Execution(e.to_string()))?;
        let k8s_ctx = self.kind_cluster.k8s_context();

        for workload_image_tag in &workload_image_tags {
            let (workload, namespace, name, image, tag) =
                workload_image_tag.workload_namespace_name_image_tag();
            cmd!(
            sh,
            "kubectl --context={k8s_ctx} set image -n {namespace} {workload}/{name} *={image}:{tag}"
        )
                .run()
                .map_err(|e| KindError::Execution(e.to_string()))?;

            cmd!(
                sh,
                "kubectl --context={k8s_ctx} rollout restart -n {namespace} {workload}/{name}"
            )
            .run()
            .map_err(|e| KindError::Execution(e.to_string()))?;

            if wait_status {
                cmd!(
            sh,
            "kubectl --context={k8s_ctx} rollout status -n {namespace} {workload}/{name} --timeout 60s"
        )
                .run()
                .map_err(|e| KindError::Execution(e.to_string()))?
            }
        }
        Ok(())
    }

    fn workload_image_tags(&self) -> Vec<WorkloadImageTag> {
        self.containers
            .iter()
            .filter(|c| c.workload.is_some())
            .map(|c| {
                let image_tag = self.image_tag(c);
                WorkloadImageTag {
                    image: image_tag.image,
                    tag: image_tag.tag,
                    workload: c.workload.clone().unwrap(),
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

impl WorkloadImageTag {
    fn workload_namespace_name_image_tag(&self) -> (&str, &str, &str, &str, &str) {
        match &self.workload {
            Workload::DaemonSet(id) => (
                "daemonset",
                id.namespace.as_str(),
                id.name.as_str(),
                self.image.as_str(),
                self.tag.as_str(),
            ),
            Workload::Deployment(id) => (
                "deployment",
                id.namespace.as_str(),
                id.name.as_str(),
                self.image.as_str(),
                self.tag.as_str(),
            ),
        }
    }
}
