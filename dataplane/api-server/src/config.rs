/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum TLSConfig {
    TLS(ServerOnlyTLSConfig),
    MutualTLS(MutualTLSConfig),
}

#[derive(Debug, Parser, Clone)]
pub struct ServerOnlyTLSConfig {
    #[clap(short, long)]
    pub server_certificate_path: PathBuf,
    #[clap(short, long)]
    pub server_private_key_path: PathBuf,
}

#[derive(Debug, Parser, Clone)]
pub struct MutualTLSConfig {
    #[clap(short, long)]
    pub server_certificate_path: PathBuf,
    #[clap(short, long)]
    pub server_private_key_path: PathBuf,
    #[clap(short, long)]
    pub client_certificate_authority_root_path: PathBuf,
}
