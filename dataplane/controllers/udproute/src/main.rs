#![allow(clippy::unnecessary_lazy_evaluations)]

use anyhow::Result;
use futures::StreamExt;
use gateway_api::apis::experimental::udproutes::UDPRoute;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{
    api::{Api, ListParams, ObjectMeta, Patch, PatchParams, Resource},
    runtime::controller::{Action, Controller},
    Client, CustomResource, ResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::BufRead, sync::Arc};
use thiserror::Error;
use tokio::time::Duration;
use tracing::*;

#[derive(Debug, Error)]
enum Error {
    #[error("Failed to create ConfigMap: {0}")]
    ConfigMapCreationFailed(#[source] kube::Error),
    #[error("MissingObjectKey: {0}")]
    MissingObjectKey(&'static str),
}

async fn reconcile(route: Arc<UDPRoute>, ctx: Arc<Data>) -> Result<Action, Error> {
    // let client = &ctx.client;
    let route_namespace = route.namespace().unwrap();
    let route_name = route.name_any();

    info!("found UDPRoute {}/{}", route_namespace, route_name);

    // TODO: build routing info and push to eBPF maps

    Ok(Action::requeue(Duration::from_secs(300)))
}

fn error_policy(_object: Arc<UDPRoute>, _error: &Error, _ctx: Arc<Data>) -> Action {
    Action::requeue(Duration::from_secs(1))
}

struct Data {
    client: Client,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let client = Client::try_default().await?;

    let udproutes = Api::<UDPRoute>::all(client.clone());

    info!("starting UDPRoute controller");

    Controller::new(udproutes, ListParams::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, Arc::new(Data { client }))
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("reconciled {:?}", o),
                Err(e) => warn!("reconcile failed: {}", e),
            }
        })
        .await;
    info!("controller terminated");
    Ok(())
}
