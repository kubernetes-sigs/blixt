/*
Copyright 2024 The Kubernetes Authors.

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

use futures::StreamExt;
use std::{sync::Arc, time::Duration};

use crate::{Context, Error, Result};

use gateway_api::apis::experimental::udproutes::UDPRoute;
use kube::{
    api::{Api, ListParams},
    runtime::{Controller, controller::Action, watcher::Config},
};
use tracing::{debug, info, warn};

pub async fn reconcile(udproute: Arc<UDPRoute>, ctx: Arc<Context>) -> Result<Action> {
    let _client = ctx.client.clone();

    let name = udproute
        .metadata
        .name
        .clone()
        .ok_or(Error::InvalidConfigError(
            "no name provided for udproute".to_string(),
        ))?;

    let ns = udproute
        .metadata
        .namespace
        .clone()
        .ok_or(Error::InvalidConfigError(
            "invalid namespace for udproute".to_string(),
        ))?;

    debug!("reconciling udproute {}/{}", ns, name);

    // TODO - implement cleanup

    // TODO - check if the route is managed by our GatewayClass

    // TODO - validation (port, protocol, etc.)

    // TODO - dataplane configuration

    info!("TODO: udproute controller unimplemented");
    Ok(Action::await_change())
}

pub async fn controller(ctx: Context) -> Result<()> {
    let udproute_api = Api::<UDPRoute>::all(ctx.client.clone());

    udproute_api
        .list(&ListParams::default())
        .await
        .map_err(Error::CRDNotFoundError)?;

    Controller::new(udproute_api, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, Arc::new(ctx))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

fn error_policy(_: Arc<UDPRoute>, error: &Error, _: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    Action::requeue(Duration::from_secs(5))
}
