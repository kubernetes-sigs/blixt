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

//! [`kind_k8s`](`crate`) provides elements that allow to build container images,
//! to create [kind](https://kind.sigs.k8s.io/) (k8s in docker) clusters and to deploy k8s resources using
//! [kustomize](https://kustomize.io/) through [kubectl](https://kubernetes.io/docs/reference/kubectl/).
//!
//! This [`crate`] depends on the host to have the following tools installed and correctly configured:
//! - `docker` or `podman`
//! - `kind`
//! - `kubectl`
//!
//! It is mainly intended for automated k8s integration tests.

pub mod infrastructure;

use std::env;
use std::path::{Path, PathBuf};

use thiserror::Error as ThisError;
use tracing::error;

use crate::infrastructure::KindClusterError;
use crate::infrastructure::KustomizeError;

/// Result typed used within the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors for the [`kind_k8s`](`crate`) crate.
///
/// The error type is used in functions that return a `Result`.
///
/// It is structured to identify the sources and to allowing matching on the according error types.
#[derive(ThisError, Debug)]
pub enum Error {
    /// Error originating from an action related to `KustomizeDeployments`
    #[error(transparent)]
    Kustomize(#[from] KustomizeError),
    /// Error originating from an action related to a `KindCluster`
    #[error(transparent)]
    Kind(#[from] KindClusterError),
    /// Error signaling an issue with the cargo workspace directory.
    #[error("Could not load CARGO_MANIFEST_DIR from environment")]
    MissingCargoManifestDir,
    /// Error signaling a missing path.
    #[error("Path {0} does not existing.")]
    PathDoesNotExist(PathBuf),
    /// Error originating from an IO operation.
    #[error("IO issue {0:?}")]
    IO(#[from] std::io::Error),
}

/// Verify if a `Path` exists and is accessible.
pub fn verify_path<T: AsRef<Path>>(path: T) -> Result<PathBuf> {
    match path.as_ref().try_exists()? {
        true => Ok(path.as_ref().to_owned()),
        false => Err(Error::PathDoesNotExist(path.as_ref().to_owned())),
    }
}

/// Get the top level cargo workspace directory from the `CARGO_MANIFEST_DIR`
/// and a `subdir` suffix that is stripped from the `CARGO_MANIFEST_DIR`.
///
/// Required to mount the workspace directory into the containers during build.
/// Allows accessing files in the workspace (e.g. the pre-built target/ folder for develop images).
pub fn cargo_workspace_dir(subdir: &str) -> Result<String> {
    let Some(workspace_dir) = env::var("CARGO_MANIFEST_DIR").ok() else {
        return Err(Error::MissingCargoManifestDir);
    };

    let Some(workspace_dir) = workspace_dir.strip_suffix(subdir) else {
        error!("Could not remove subdirectory {subdir} from CARGO_MANIFEST_DIR {workspace_dir}");
        return Err(Error::MissingCargoManifestDir);
    };

    verify_path(workspace_dir)?;
    Ok(workspace_dir.to_string())
}
