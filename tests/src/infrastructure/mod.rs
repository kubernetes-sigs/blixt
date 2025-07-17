mod container_images;
mod kind_cluster;
mod kustomize;

use std::env;
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;

pub use container_images::Container;
pub use container_images::ContainerImages;
pub use container_images::ImageAction;
pub use container_images::ImageError;
pub use kind_cluster::KindCluster;
pub use kind_cluster::KindError;
pub use kustomize::KustomizeDeployments;
pub use kustomize::KustomizeError;

use crate::{Error, Result, TestMode};
use controlplane::NamespacedName;

#[derive(Clone, Debug)]
pub enum ContainerState {
    Running,
    Exited,
    NotFound,
}

#[derive(Default, Clone)]
pub enum ContainerRuntime {
    #[default]
    Podman,
    Docker,
}

#[derive(Clone, Debug)]
pub enum Workload {
    DaemonSet(NamespacedName),
    Deployment(NamespacedName),
}

pub fn default_containers(mode: TestMode, cargo_workspace_dir: &str) -> Result<Vec<Container>> {
    let containerfile_dir = match mode {
        TestMode::Development => "build/develop",
        TestMode::Release => "build",
    };

    Ok(vec![
        {
            let app = "dataplane";
            Container {
                containerfile: verify_file_path(format!(
                    "{cargo_workspace_dir}/{containerfile_dir}/Containerfile.{app}"
                ))?,
                image_name: format!("blixt-{app}"),
                workload: Some(Workload::DaemonSet(NamespacedName {
                    namespace: "blixt-system".to_string(),
                    name: format!("blixt-{app}"),
                })),
            }
        },
        // FIXME: oder for containers MUST not affect results
        // currently if dataplane is restarted after controlplane the EBPF tables stay empty
        // as controlplane configures the old dataplane daemonset, ideally this is fixed on the EBFP level
        // by not initializing the according maps on startup if present on the system
        // needs to be checked if this is possible, but would allow for continuous operation during
        // dataplane rollouts
        {
            let app = "controlplane";
            Container {
                containerfile: verify_file_path(format!(
                    "{cargo_workspace_dir}/{containerfile_dir}/Containerfile.{app}"
                ))?,
                image_name: format!("blixt-{app}"),
                workload: Some(Workload::Deployment(NamespacedName {
                    namespace: "blixt-system".to_string(),
                    name: format!("blixt-{app}"),
                })),
            }
        },
        {
            let app = "udp_test_server";
            Container {
                containerfile: verify_file_path(format!(
                    "{cargo_workspace_dir}/{containerfile_dir}/Containerfile.{app}"
                ))?,
                image_name: format!("blixt-{app}"),
                workload: None,
            }
        },
    ])
}

fn verify_file_path(containerfile: String) -> Result<PathBuf> {
    let path = PathBuf::from(containerfile);
    match path.try_exists()? {
        true => Ok(path),
        false => Err(Error::PathDoesNotExist(path.clone())),
    }
}

pub fn cargo_workspace_dir(sub_dir: &str) -> Result<String> {
    let Some(workspace_dir) = env::var("CARGO_MANIFEST_DIR").ok() else {
        return Err(Error::MissingCargoManifestDir);
    };
    let Some(workspace_dir) = workspace_dir.strip_suffix(sub_dir) else {
        return Err(Error::MissingCargoManifestDir);
    };
    Ok(workspace_dir.to_string())
}

impl Display for ContainerRuntime {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerRuntime::Podman => f.write_str("podman"),
            ContainerRuntime::Docker => f.write_str("docker"),
        }
    }
}

impl Debug for ContainerRuntime {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}
