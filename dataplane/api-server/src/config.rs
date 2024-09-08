/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone)]
pub struct GrpcConfig {
    #[clap(long, default_value = "false")]
    pub enable_tls: bool,
    #[clap(long, default_value = "false")]
    pub enable_mtls: bool,
    pub certificate_authority_root_path: Option<PathBuf>,
    pub server_certificate_path: Option<PathBuf>,
    pub server_private_key_path: Option<PathBuf>,
    pub client_certificate_authority_root_path: Option<PathBuf>,
    pub client_certificate_path: Option<PathBuf>,
    pub client_private_key_path: Option<PathBuf>,
}

impl GrpcConfig {
    pub fn validate(&self) -> Result<(), String> {
        fn validate_paths(paths: &[(&Option<PathBuf>, &str)]) -> Result<(), String> {
            for (path, name) in paths {
                if path.is_none() {
                    return Err(format!("Missing required path: {}", name));
                }
            }
            Ok(())
        }

        let tls_paths = &[
            (
                &self.certificate_authority_root_path,
                "certificate_authority_root_path",
            ),
            (&self.server_certificate_path, "server_certificate_path"),
            (&self.server_private_key_path, "server_private_key_path"),
        ];

        let mtls_paths = &[
            (
                &self.client_certificate_authority_root_path,
                "client_certificate_authority_root_path",
            ),
            (&self.client_certificate_path, "client_certificate_path"),
            (&self.client_private_key_path, "client_private_key_path"),
        ];

        if self.enable_mtls {
            validate_paths(tls_paths)?;
            validate_paths(mtls_paths)?;
        } else if self.enable_tls {
            validate_paths(tls_paths)?;
        }

        Ok(())
    }
}
