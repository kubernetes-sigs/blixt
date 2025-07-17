pub mod infrastructure;

use crate::infrastructure::KustomizeError;
use crate::infrastructure::{ImageError, KindError};
use controlplane::K8sError;
use std::path::PathBuf;
use thiserror::Error as ThisError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    Kustomize(#[from] KustomizeError),
    #[error(transparent)]
    Kind(#[from] KindError),
    #[error(transparent)]
    ImageError(#[from] ImageError),
    #[error("Could not load CARGO_MANIFEST_DIR from environment")]
    MissingCargoManifestDir,
    #[error("Path {0} does not existing.")]
    PathDoesNotExist(PathBuf),
    #[error(transparent)]
    K8s(#[from] K8sError),
    #[error(transparent)]
    Controlplane(#[from] controlplane::Error),
    #[error("IO issue {0:?}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub enum TestMode {
    Development,
    Release,
}
