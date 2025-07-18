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

use kube::Client;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::ops::Sub;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use api_server::backends::{Target, Targets, Vip};
use futures::StreamExt;
use gateway_api::apis::experimental::tcproutes::{
    TCPRoute, TCPRouteParentRefs, TCPRouteRulesBackendRefs,
};
use gateway_api::apis::standard::gateways::Gateway;
use gateway_api::gatewayclasses::GatewayClass;
use k8s_openapi::api::core::v1::Endpoints;
use kube::api::{Patch, PatchParams};
use kube::runtime::Controller;
use kube::runtime::controller::Action;
use kube::runtime::watcher::Config;
use kube::{Api, Resource, ResourceExt};
use serde_json::json;
use thiserror::Error as ThisError;
use tracing::log::error;
use tracing::{debug, info, warn};

use crate::consts::{BLIXT_FIELD_MANAGER, DATAPLANE_FINALIZER, GATEWAY_CLASS_CONTROLLER_NAME};
use crate::controllers::gateway::get_gateway_ips;
use crate::dataplane::DataplaneClientManager;
use crate::{Error, NamespaceName};
use crate::{K8sError, NamespacedName, Result};

#[derive(Clone)]
pub struct TCPRouteController {
    dataplane_client: DataplaneClientManager,
    k8s_client: Client,
}

#[derive(ThisError, Debug)]
pub enum TCPRouteError {
    #[error(transparent)]
    K8s(#[from] K8sError),
    #[error("{0:?} found too many Gateways {1}. Currently only a single Gateway is supported.")]
    TooManyGatewaysFound(NamespacedName, usize),
    #[error("TCPRoute {0:?} does not have healthy backends.")]
    NoHealthyBackends(NamespacedName),
    #[error("Gateway {0:?} IPv6 not supported.")]
    GatewayIPv6NotSupported(NamespacedName),
    #[error("{0:?} no matching port found")]
    NoMatchingGatewayPort(NamespacedName),
}

impl TCPRouteController {
    pub fn new(k8s_ctx: Client, dataplane_client: DataplaneClientManager) -> Self {
        Self {
            dataplane_client,
            k8s_client: k8s_ctx,
        }
    }

    pub async fn start(self) -> Result<()> {
        let tcproute_api = Api::<TCPRoute>::all(self.k8s_client.clone());

        Controller::new(tcproute_api, Config::default().any_semantic())
            .shutdown_on_signal()
            .run(Self::reconcile, Self::error_policy, Arc::new(self))
            .filter_map(|x| async move { std::result::Result::ok(x) })
            .for_each(|_| futures::future::ready(()))
            .await;

        Ok(())
    }

    async fn reconcile(tcp_route: Arc<TCPRoute>, ctx: Arc<Self>) -> Result<Action> {
        error!("TCPRoute: {tcp_route:?}");
        let start = Instant::now();

        let tcp_route_id = tcp_route.metadata.namespaced_name()?;
        let (parent_refs, backend_refs) = Self::validate_tcp_route(&tcp_route)?;

        // TODO: support multiple gateways, the TCPRoute spec allows for multiple parents
        // as of now the function returns an error when multiple gateways are found
        let managed_gateways = ctx.managed_route(&tcp_route_id, parent_refs).await?;
        if managed_gateways.is_empty() {
            // TODO: enable orphan checking
            return Ok(Action::requeue(Duration::from_secs(5)));
        };

        if !tcp_route
            .finalizers()
            .contains(&DATAPLANE_FINALIZER.to_string())
        {
            if tcp_route.meta().deletion_timestamp.is_some() {
                // already handled
                return Ok(Action::await_change());
            }

            ctx.set_dataplane_finalizer(&tcp_route).await?;
        };

        // if the TCPRoute is being deleted, remove it from the DataPlane
        // TODO: add deletion grace period
        if tcp_route.meta().deletion_timestamp.is_some() {
            for gateway in managed_gateways.iter() {
                ctx.ensure_tcp_route_deleted_in_dataplane(&tcp_route, gateway)
                    .await?;
            }
            ctx.remove_dataplane_finalizer(&tcp_route).await?;
            return Ok(Action::await_change());
        }

        // in all other cases ensure the TCPRoute is configured in the dataplane
        for gateway in managed_gateways.iter() {
            ctx.ensure_tcp_route_configure_in_dataplane(
                &tcp_route_id,
                parent_refs,
                gateway,
                &backend_refs,
            )
            .await?;
        }

        let duration = Instant::now().sub(start);
        info!("Finished reconciling in {:?} ms", duration.as_millis());
        Ok(Action::await_change())
    }

    fn validate_tcp_route(
        tcp_route: &TCPRoute,
    ) -> Result<(&Vec<TCPRouteParentRefs>, Vec<&TCPRouteRulesBackendRefs>)> {
        let tcp_route_id = &tcp_route.metadata.namespaced_name()?;

        let Some(parent_refs) = &tcp_route.spec.parent_refs else {
            return Err(
                K8sError::missing_resource_property(tcp_route_id, "spec.parent_refs").into(),
            );
        };
        if parent_refs.is_empty() {
            return Err(K8sError::empty_resource_property(tcp_route_id, "spec.parent_refs").into());
        }
        if tcp_route.spec.rules.is_empty() {
            return Err(K8sError::empty_resource_property(tcp_route_id, "spec.rules").into());
        };
        let backend_refs = tcp_route
            .spec
            .rules
            .iter()
            .filter_map(|r| r.backend_refs.as_ref())
            .flatten()
            .collect::<Vec<_>>();

        if backend_refs.is_empty() {
            return Err(K8sError::empty_resource_property(
                tcp_route_id,
                "spec.rules.backend_refs[]",
            )
            .into());
        };

        Ok((parent_refs, backend_refs))
    }

    async fn ensure_tcp_route_configure_in_dataplane(
        &self,
        tcp_route_id: &NamespacedName,
        parent_refs: &[TCPRouteParentRefs],
        gateway: &Gateway,
        backend_refs: &[&TCPRouteRulesBackendRefs],
    ) -> Result<()> {
        let targets = self
            .compile_tcp_route_to_data_plane_targets(
                tcp_route_id,
                parent_refs,
                backend_refs,
                gateway,
            )
            .await?;

        debug!(
            "Updating targets for TCPRoute {}: {targets:?}",
            tcp_route_id
        );
        self.dataplane_client.update_targets(targets).await
    }

    async fn ensure_tcp_route_deleted_in_dataplane(
        &self,
        tcp_route: &TCPRoute,
        gateway: &Gateway,
    ) -> Result<()> {
        let (parent_refs, _) = Self::validate_tcp_route(tcp_route)?;

        let gateway_ips = get_gateway_ips(gateway)?;
        // TODO: multiple gateways and IPv6 support
        let gw_ip: Ipv4Addr = match gateway_ips[0] {
            IpAddr::V4(v4) => v4,
            IpAddr::V6(_) => {
                return Err(TCPRouteError::GatewayIPv6NotSupported(
                    tcp_route.metadata.namespaced_name()?,
                )
                .into());
            }
        };

        if let Some(port) = parent_refs[0].port {
            debug!(
                "Removing Vip for TCPRoute {}: {:?}:{port}",
                tcp_route.metadata.namespaced_name()?,
                gw_ip
            );
            self.dataplane_client
                .delete_vip(Vip {
                    ip: gw_ip.to_bits(),
                    port: port as u32,
                })
                .await
        } else {
            Err(TCPRouteError::NoMatchingGatewayPort(tcp_route.metadata.namespaced_name()?).into())
        }
    }

    fn error_policy(_: Arc<TCPRoute>, error: &crate::Error, _: Arc<TCPRouteController>) -> Action {
        warn!("Failed to reconcile: {:?}", error);
        Action::requeue(Duration::from_secs(5))
    }

    async fn set_dataplane_finalizer(&self, tcp_route: &TCPRoute) -> Result<()> {
        let mut finalizers = tcp_route
            .finalizers()
            .iter()
            .cloned()
            .collect::<HashSet<String>>();
        finalizers.insert(DATAPLANE_FINALIZER.to_string());
        let finalizers = finalizers.into_iter().collect::<Vec<String>>();

        debug!(
            "Setting dataplane finalizers {:?} for TCPRoute {}",
            &finalizers,
            tcp_route.metadata.namespaced_name()?
        );
        self.apply_finalizers(tcp_route, finalizers).await
    }

    async fn remove_dataplane_finalizer(&self, tcp_route: &TCPRoute) -> Result<()> {
        let finalizers = tcp_route
            .finalizers()
            .iter()
            .filter(|f| *f != DATAPLANE_FINALIZER)
            .cloned()
            .collect::<Vec<String>>();

        debug!(
            "Removing dataplane finalizer for TCPRoute {}",
            tcp_route.metadata.namespaced_name()?
        );
        self.apply_finalizers(tcp_route, finalizers).await
    }

    async fn apply_finalizers(
        &self,
        tcp_route: &TCPRoute,
        finalizers: Vec<String>,
    ) -> Result<(), Error> {
        let tcp_route_id = tcp_route.metadata.namespaced_name()?;

        let pp = PatchParams::apply(BLIXT_FIELD_MANAGER);
        let patch = Patch::Apply(json!({
            "apiVersion": TCPRoute::api_version(&()),
            "kind": TCPRoute::kind(&()),
            "metadata": {
                "finalizers": Some(finalizers.clone()),
            }
        }));

        let tcp_route_api: Api<TCPRoute> =
            Api::namespaced(self.k8s_client.clone(), &tcp_route_id.namespace);
        tcp_route_api
            .patch_metadata(&tcp_route_id.name, &pp, &patch)
            .await
            .map_err(|e| K8sError::client(e).into())
            .map(|_| ())
    }

    // TODO: currently errors on > 1 Gateways found
    // add support for multiple Gateways
    async fn managed_route(
        &self,
        route_identifier: &NamespacedName,
        parent_refs: &[TCPRouteParentRefs],
    ) -> Result<Vec<Gateway>> {
        let mut managed_gateways: Vec<Gateway> = vec![];
        let gateway_class_api: Api<GatewayClass> = Api::all(self.k8s_client.clone());
        for parent_ref in parent_refs {
            let namespace = parent_ref
                .namespace
                .clone()
                .unwrap_or(route_identifier.namespace.clone());
            let gateway_name = parent_ref.name.as_str();
            let gateway_api: Api<Gateway> = Api::namespaced(self.k8s_client.clone(), &namespace);

            let gateway = match gateway_api.get(gateway_name).await {
                Ok(gw) => gw,
                Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                    warn!(
                        "Fetching Gateway {}/{} kubernetes API error 404",
                        &namespace, &gateway_name
                    );
                    continue;
                }
                Err(e) => return Err(K8sError::client(e).into()),
            };

            let gateway_class = match gateway_class_api
                .get(&gateway.spec.gateway_class_name)
                .await
            {
                Ok(gwc) => gwc,
                Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                    warn!(
                        "Fetching GatewayClass {} kubernetes API error 404",
                        &gateway.spec.gateway_class_name
                    );
                    continue;
                }
                Err(e) => return Err(K8sError::client(e).into()),
            };

            if gateway_class.spec.controller_name != GATEWAY_CLASS_CONTROLLER_NAME {
                // not managed by this implementation, check the next parent ref
                continue;
            }

            if let Some(port) = parent_ref.port {
                if !gateway
                    .spec
                    .listeners
                    .iter()
                    .any(|listener| listener.port == port && listener.protocol == "TCP")
                {
                    continue;
                }
            }

            managed_gateways.push(gateway)
        }

        // TODO: support multiple gateways
        if managed_gateways.len() > 1 {
            return Err(TCPRouteError::TooManyGatewaysFound(
                route_identifier.clone(),
                managed_gateways.len(),
            )
            .into());
        }

        Ok(managed_gateways)
    }

    async fn compile_tcp_route_to_data_plane_targets(
        &self,
        tcp_route_id: &NamespacedName,
        parent_refs: &[TCPRouteParentRefs],
        backend_refs: &[&TCPRouteRulesBackendRefs],
        gateway: &Gateway,
    ) -> Result<Targets> {
        let gateway_port =
            Self::get_gateway_port_for_parent_refs(tcp_route_id, parent_refs, gateway)?;
        let gateway_ips = get_gateway_ips(gateway)?;
        // TODO: multiple gateways and IPv6 support
        let gw_ip: Ipv4Addr = match gateway_ips[0] {
            IpAddr::V4(v4) => v4,
            IpAddr::V6(_) => {
                return Err(TCPRouteError::GatewayIPv6NotSupported(tcp_route_id.clone()).into());
            }
        };

        let mut backend_targets: Vec<(Ipv4Addr, u16)> = vec![];
        for backend_ref in backend_refs {
            let backend_namespace = backend_ref
                .namespace
                .as_deref()
                .unwrap_or(tcp_route_id.namespace.as_str());
            let backend_name = backend_ref.name.clone();
            let backend_port = backend_ref.port.unwrap_or(80);

            let endpoint_api =
                Api::<Endpoints>::namespaced(self.k8s_client.clone(), backend_namespace);
            let endpoints = endpoint_api
                .get(backend_name.as_str())
                .await
                .map_err(K8sError::client)?;

            for subset in endpoints.subsets.unwrap_or_default() {
                for address in subset.addresses.unwrap_or_default() {
                    if let Ok(pod_ip) = IpAddr::from_str(&address.ip) {
                        match pod_ip {
                            IpAddr::V4(ip) => {
                                backend_targets.push((ip, backend_port as u16));
                            }
                            IpAddr::V6(_) => { /* TODO: support IPv6 */ }
                        }
                    }
                }
            }
        }

        if backend_targets.is_empty() {
            return Err(TCPRouteError::NoHealthyBackends(tcp_route_id.clone()).into());
        }

        info!(
            "Targets {{ vip: {{ ip {:?}, port: {} }}, targets: {:?}}}",
            gw_ip, gateway_port, backend_targets
        );
        Ok(Targets {
            vip: Some(Vip {
                ip: gw_ip.to_bits(),
                port: gateway_port as u32,
            }),
            targets: backend_targets
                .into_iter()
                .map(|(ip, port)| Target {
                    daddr: ip.to_bits(),
                    dport: port as u32,
                    ifindex: None,
                })
                .collect::<Vec<Target>>(),
        })
    }

    fn get_gateway_port_for_parent_refs(
        tcp_route_id: &NamespacedName,
        parent_refs: &[TCPRouteParentRefs],
        gateway: &Gateway,
    ) -> Result<i32> {
        for parent_ref in parent_refs {
            if let Some(port) = parent_ref.port {
                for listener in &gateway.spec.listeners {
                    if listener.port == port {
                        return Ok(port);
                    }
                }
            }
        }

        Err(TCPRouteError::NoMatchingGatewayPort(tcp_route_id.clone()).into())
    }
}
