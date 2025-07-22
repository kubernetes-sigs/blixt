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

//! Contains structs to create clusters, container images and k8s resources.
//! The resources can be connected and allow for integration.
//!
//! For example to create a [`KindCluster`], build [`ContainerImages`], load them into
//! the [`KindCluster`] and deploy a set of k8s resources through a [`KustomizeDeployments`].

mod container_images;
mod kind_cluster;
mod kustomize;

pub use container_images::Container;
pub use container_images::ContainerImageAction;
pub use container_images::ContainerImageError;
pub use container_images::ContainerImages;
pub use kind_cluster::KindCluster;
pub use kind_cluster::KindClusterError;
pub use kustomize::KustomizeDeployments;
pub use kustomize::KustomizeError;

use std::ffi::OsStr;
use std::fmt::{Debug, Display, Formatter};
use std::path::Path;
use std::{env, io};

use thiserror::Error;
use tokio::process::Command;
use tracing::{error, info};

/// Represents a Container state.
///
/// Refers to the containers `.State` property (in case a container was found).
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq)]
pub enum ContainerState {
    Running,
    Exited,
    NotFound,
}

/// Host container runtime to be used for image builds and kind clusters.
#[allow(missing_docs)]
#[derive(Default, Clone)]
pub enum ContainerRuntime {
    #[default]
    Podman,
    Docker,
}

/// K8s workload type with a corresponding identifier.
#[allow(missing_docs)]
#[derive(Clone)]
pub enum Workload {
    DaemonSet(NamespacedName),
    Deployment(NamespacedName),
}

/// Fully qualified image name including tag.
#[allow(missing_docs)]
#[derive(Debug)]
pub struct ImageTag {
    /// image fully qualified name
    pub image: String,
    pub tag: String,
}

/// A [`Workload`] with an optional image tag.
///
/// In case an image tag is available the workload image references
/// will be updated to the according `image:tag` before an action is executed
/// e.g. the Deployments image field is updated, and then a rollout restart is carried out
#[derive(Debug)]
pub struct WorkloadImageTag {
    /// workload identifier
    pub id: Workload,
    /// optional image tag
    pub image_tag: Option<ImageTag>,
}

impl WorkloadImageTag {
    fn image_tag(&self) -> Option<(&str, &str)> {
        self.image_tag
            .as_ref()
            .map(|it| (it.image.as_str(), it.tag.as_str()))
    }
    fn workload_namespace_name(&self) -> (&str, &str, &str) {
        self.id.workload_namespace_name()
    }
}

impl Workload {
    fn workload_namespace_name(&self) -> (&str, &str, &str) {
        match &self {
            Workload::DaemonSet(id) => ("daemonset", id.namespace.as_str(), id.name.as_str()),
            Workload::Deployment(id) => ("deployment", id.namespace.as_str(), id.name.as_str()),
        }
    }
}

/// Wraps a `tokio::process::Command` for easier handling.
///
/// The crate uses this struct for all executions, which allows to use helpers
/// like `tokio::try_join` to e.g. parallelize image build and cluster creation.
pub struct AsyncCommand {
    cmd: Command,
}

/// Errors originating from [`AsyncCommand`].
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum AsyncCommandError {
    #[error("Failed spawning the command: {0:?}")]
    Spawn(io::Error),
    #[error("Failed to wait for the command: {0:?}")]
    Wait(io::Error),
    #[error("Failed getting output for the command: {0:?}")]
    Output(io::Error),
    #[error("Command exited with {0:?}")]
    ExitStatus(Option<i32>),
}

impl AsyncCommand {
    /// create a new AsyncCommand by providing the command binary and the arguments
    pub fn new<C: AsRef<OsStr>, A: AsRef<OsStr>>(cmd: C, args: &[A]) -> Self {
        let mut cmd = Command::new(cmd);
        {
            args.iter().for_each(|a| {
                let _ = &mut cmd.arg(a);
            });
        }
        Self { cmd }
    }

    /// update the commands environment providing a slice of key/value pairs
    pub fn env<D: IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)>>(
        mut self,
        env: D,
    ) -> Self {
        env.into_iter().for_each(|(k, v)| {
            let _ = self.cmd.env(k, v);
        });
        self
    }

    /// set the working directory for the commands execution
    pub fn working_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.cmd.current_dir(dir);
        self
    }

    async fn run(&mut self) -> Result<(), AsyncCommandError> {
        info!("run: {:?}", self.cmd);
        let exit_status = self
            .cmd
            .spawn()
            .map_err(AsyncCommandError::Spawn)?
            .wait()
            .await
            .map_err(AsyncCommandError::Wait)?;

        if !exit_status.success() {
            return Err(AsyncCommandError::ExitStatus(exit_status.code()));
        }

        Ok(())
    }

    async fn read(&mut self) -> Result<(String, String), AsyncCommandError> {
        info!("read: {:?}", self.cmd);
        let output = self.cmd.output().await.map_err(AsyncCommandError::Output)?;

        if !output.status.success() {
            return Err(AsyncCommandError::ExitStatus(output.status.code()));
        }

        let stdout = String::from_utf8_lossy(output.stdout.as_slice()).to_string();
        let stderr = String::from_utf8_lossy(output.stderr.as_slice()).to_string();

        Ok((stdout, stderr))
    }
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

impl Display for Workload {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let id = match self {
            Workload::DaemonSet(id) => {
                f.write_str("daemonset")?;
                id
            }
            Workload::Deployment(id) => {
                f.write_str("deployment")?;
                id
            }
        };
        f.write_str(" ")?;
        Display::fmt(id, f)
    }
}

impl Debug for Workload {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

impl AsRef<WorkloadImageTag> for WorkloadImageTag {
    fn as_ref(&self) -> &WorkloadImageTag {
        self
    }
}

impl AsRef<Workload> for Workload {
    fn as_ref(&self) -> &Workload {
        self
    }
}

/// K8s identifier consisting of a namespace and a name.
#[allow(missing_docs)]
#[derive(Clone)]
pub struct NamespacedName {
    pub namespace: String,
    pub name: String,
}

impl Display for NamespacedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.namespace.as_str())?;
        f.write_str("/")?;
        f.write_str(self.name.as_str())
    }
}

impl Debug for NamespacedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

pub(crate) fn container_host_env(container_host: Option<&String>) -> Vec<(String, String)> {
    let container_host = container_host.cloned().unwrap_or(
        env::var("CONTAINER_HOST")
            .ok()
            .unwrap_or("unix:///run/podman/podman.sock".to_string()),
    );

    vec![("CONTAINER_HOST".to_string(), container_host.to_string())]
}
