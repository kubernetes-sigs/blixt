use kube::Client;
use std::net::{IpAddr, Ipv4Addr};
use std::ops::Sub;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::client_manager::DataplaneClientManager;
use crate::consts::{DATAPLANE_FINALIZER, GATEWAY_CLASS_CONTROLLER_NAME};
use crate::controllers::NamespaceName;
use crate::controllers::get_gateway_ips;
use crate::{K8sError, Result};

use api_server::backends::{Target, Targets, Vip};
use futures::StreamExt;
use gateway_api::apis::experimental::tcproutes::{
    TCPRoute, TCPRouteParentRefs, TCPRouteRulesBackendRefs, TCPRouteSpec,
};
use gateway_api::apis::standard::gateways::{Gateway, GatewaySpec};
use gateway_api::gatewayclasses::{GatewayClass, GatewayClassSpec};
use k8s_openapi::api::core::v1::{Endpoints, Service};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{ListParams, Patch, PatchParams};
use kube::runtime::Controller;
use kube::runtime::controller::Action;
use kube::runtime::watcher::Config;
use kube::{Api, Error as KubeError, Resource, ResourceExt};
use thiserror::Error as ThisError;
use tracing::log::error;
use tracing::{info, warn};

#[derive(Clone)]
pub struct TCPRouteController {
    dataplane_client: DataplaneClientManager,
    k8s_client: Client,
}

#[derive(ThisError, Debug)]
pub enum TCPRouteError {
    #[error(transparent)]
    K8s(#[from] K8sError),
    #[error("Gateway {0}/{1} had {2} IP addresses, currently only a single is supported")]
    OnlySingleGatewayIpAddressSupported(String, String, usize),
    #[error("Gateway {0}/{1} has no addresses.")]
    GatewayNoStatus(String, String),
    #[error("Gateway {0}/{1} has no addresses.")]
    GatewayNoIpAddress(String, String),
    #[error("Gateway {0}/{1} has no IPv4 address.")]
    GatewayNoIPv4Address(String, String),
    #[error("Gateway {0}/{1} fetching failed with error {2}.")]
    GatewayFetchFailed(String, String, kube::Error),
    #[error("TCPRoute {0}/{1} has no parentRefs.")]
    TCPRouteNoParentRefs(String, String),
    #[error("TCPRoute {0}/{1} has no rules.")]
    TCPRouteNoRules(String, String),
    #[error("TCPRoute {0}/{1} rule has no backendRefs.")]
    TCPRouteRulesMissingBackendRef(String, String),
    #[error("TCPRoute {0}/{1} backendRefs do not have a Service associated.")]
    TCPRouteBackendRefsNoService(String, String),
    #[error("Gateway {0}/{1} Listener not found for parentRef {2} with port {3}")]
    GatewayListenerNotFound(String, String, String, i32),
    #[error("TCPRoute {0}/{1} parentRef {2} does not have a port associated.")]
    TcpRouteParentRefPortMissing(String, String, String),
    #[error("backendRef {0}/{1} does not have a port associated.")]
    BackendRefPortMissing(String, String),
    #[error("backendRef {0}/{1} could not locate a matching Service port.")]
    BackendRefTargetPortMissing(String, String),
    #[error(
        "TCPRoute {0}/{1} found too many Gateways {2}. Currently only a single Gateway is supported."
    )]
    TooManyGatewaysFound(String, String, usize),
    #[error(
        "TCPRoute {0}/{1} found too many parentRefs {2}. Currently only a single parentRef is supported."
    )]
    TCPRouteTooManyParentRefs(String, String, usize),
    #[error("TCPRoute {0}/{1} endpoint {2} address not ready.")]
    TCPRouteEndpointAddressNotReady(String, String, String),
    #[error("TCPRoute {0}/{1} endpoint {2} port not ready.")]
    TCPRouteEndpointPortNotReady(String, String, String),
    #[error("TCPRoute {0}/{1} endpoint {2} has no addresses.")]
    TCPRouteEndpointNoAddresses(String, String, String),
    #[error("TCPRoute {0}/{1} endpoint {2} address is empty.")]
    TCPRouteEndpointAddressEmpty(String, String, String),
    #[error("TCPRoute {0}/{1} does not have healthy backends.")]
    TCPRouteNoHealthyBackends(String, String),
    #[error("Service {0}/{1} does not have a spec.")]
    ServiceSpecMissing(String, String),
    #[error("Service {0}/{1} does not have a ports associated.")]
    ServicePortsMissing(String, String),
    #[error("Gateway {0}/{1} IPv not supported.")]
    GatewayIPv6NotSupported(String, String),
    #[error("Service {0}/{1} failed to parse target port {2} {3}")]
    ServiceTargetPortParseError(String, String, String, String),
    #[error("{0}/{1} no matching port found")]
    NoMatchingGatewayPort(String, String),
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
        tcproute_api
            .list(&ListParams::default().limit(1))
            .await
            .map_err(K8sError::Client)?; // TODO: map not found

        Controller::new(tcproute_api, Config::default().any_semantic())
            .shutdown_on_signal()
            .run(Self::reconcile, Self::error_policy, Arc::new(self))
            .filter_map(|x| async move { std::result::Result::ok(x) })
            .for_each(|_| futures::future::ready(()))
            .await;

        Ok(())
    }

    pub async fn reconcile(tcp_route: Arc<TCPRoute>, ctx: Arc<Self>) -> Result<Action> {
        error!("TCPRoute: {tcp_route:?}");
        let start = Instant::now();
        // TODO: check if object still exists

        // TODO: support multiple gateways
        // as of now the function returns an error when multiple gateways are found
        let managed_gateways = ctx.managed_route(&tcp_route).await?;
        if managed_gateways.is_empty() {
            // TODO: enable orphan checking
            return Ok(Action::await_change());
        };

        if !tcp_route
            .finalizers()
            .contains(&DATAPLANE_FINALIZER.to_string())
        {
            if tcp_route.meta().deletion_timestamp.is_some() {
                // if the finalizer isn't set, AND the object is being deleted then there's
                // no reason to bother with dataplane configuration for it its already
                // handled.
                return Ok(Action::await_change());
            }
            // if the finalizer is not set, and the object is not being deleted, set the
            // finalizer before we do anything else to ensure we don't lose track of
            // dataplane configuration.
            ctx.set_dataplane_finalizer(&tcp_route).await?;
        };

        // if the TCPRoute is being deleted, remove it from the DataPlane
        // TODO: enable deletion grace period
        if tcp_route.meta().deletion_timestamp.is_some() {
            for gateway in managed_gateways.iter() {
                ctx.ensure_tcp_route_deleted_in_dataplane(&tcp_route, gateway)
                    .await?;
            }
            ctx.remove_dataplane_finalizer(&tcp_route).await?;
        }

        // in all other cases ensure the TCPRoute is configured in the dataplane
        for gateway in managed_gateways.iter() {
            ctx.ensure_tcp_route_configure_in_dataplane(&tcp_route, gateway)
                .await?;
        }

        let duration = Instant::now().sub(start);
        info!("finished reconciling in {:?} ms", duration.as_millis());
        Ok(Action::await_change())
    }

    async fn ensure_tcp_route_configure_in_dataplane(
        &self,
        tcp_route: &TCPRoute,
        gateway: &Gateway,
    ) -> Result<()> {
        let targets = self
            .compile_tcproute_to_data_plane_backend(tcp_route, gateway)
            .await?;

        info!("Updating targets: {targets:?}");
        self.dataplane_client.update_targets(targets).await?;
        info!("successfully updated dataplane");
        Ok(())
    }

    async fn ensure_tcp_route_deleted_in_dataplane(
        &self,
        _tcp_route: &TCPRoute,
        _gateway: &Gateway,
    ) -> Result<()> {
        todo!()
    }

    fn error_policy(_: Arc<TCPRoute>, error: &crate::Error, _: Arc<TCPRouteController>) -> Action {
        warn!("reconcile failed: {:?}", error);
        Action::requeue(Duration::from_secs(5))
    }

    async fn set_dataplane_finalizer(&self, tcp_route: &TCPRoute) -> Result<()> {
        let namespace = tcp_route.metadata.namespace()?;
        let tcp_route_name = tcp_route.metadata.name()?;

        let metadata = ObjectMeta {
            finalizers: Some(vec![DATAPLANE_FINALIZER.to_string()]),
            ..Default::default()
        };

        let tcp_route_api: Api<TCPRoute> =
            Api::namespaced(self.k8s_client.clone(), namespace.as_str());
        let pp = PatchParams::default();

        tcp_route_api
            .patch_metadata(&tcp_route_name, &pp, &Patch::Merge(metadata))
            .await
            .map_err(|e| K8sError::Client(e).into()) // FIXME: this looks strange
            .map(|_| ())
    }

    async fn remove_dataplane_finalizer(&self, tcp_route: &TCPRoute) -> Result<()> {
        let namespace = tcp_route.metadata.namespace()?;
        let tcp_route_name = tcp_route.metadata.name()?;

        let finalizers = tcp_route
            .finalizers()
            .iter()
            .filter(|f| *f != crate::consts::DATAPLANE_FINALIZER)
            .cloned()
            .collect::<Vec<String>>();

        let metadata = ObjectMeta {
            finalizers: Some(finalizers),
            ..Default::default()
        };

        let tcp_route_api: Api<TCPRoute> = Api::namespaced(self.k8s_client.clone(), &namespace);
        let pp = PatchParams::apply(crate::consts::BLIXT_FIELD_MANAGER);

        tcp_route_api
            .patch_metadata(&tcp_route_name, &pp, &Patch::Apply(&metadata))
            .await
            .map_err(|e| K8sError::Client(e).into())
            .map(|_| ())
    }

    async fn managed_route(&self, tcp_route: &TCPRoute) -> Result<Vec<Gateway>> {
        let tcp_route_spec: &TCPRouteSpec = &tcp_route.spec;
        let namespace = tcp_route.metadata.namespace()?;
        let route_name = tcp_route.metadata.name()?;

        let Some(parent_refs) = &tcp_route_spec.parent_refs else {
            return Err(TCPRouteError::TCPRouteNoParentRefs(namespace, route_name).into());
        };

        if tcp_route.spec.rules.is_empty() {
            return Err(TCPRouteError::TCPRouteNoRules(namespace, route_name).into());
        };

        let mut supported_gateways: Vec<Gateway> = vec![];
        let gateway_class_api: Api<GatewayClass> = Api::all(self.k8s_client.clone());
        for parent in parent_refs {
            let namespace = parent.namespace.clone().unwrap_or(namespace.clone());
            let gateway_api: Api<Gateway> = Api::namespaced(self.k8s_client.clone(), &namespace);

            // Get Gateway for TCP Route
            //let gateway_res : std::result::Result<Gateway, KubeError> = gateway_api.get(parent.name.as_str()).await;
            let gateway_res: std::result::Result<Gateway, KubeError> =
                gateway_api.get(parent.name.as_str()).await;
            if gateway_res.is_err() {
                let e = gateway_res.err().unwrap();
                match &e {
                    KubeError::Api(api) => {
                        if api.code == 404 {
                            warn!(
                                "Fetching Gateway {}/{} kubernetes API error 404",
                                &parent.name, &namespace
                            );
                            continue;
                        }
                    }
                    _ => {
                        return Err(TCPRouteError::GatewayFetchFailed(
                            namespace,
                            parent.name.clone(),
                            e,
                        )
                        .into());
                    }
                };
            } else {
                let gateway = gateway_res.unwrap();
                let gateway_spec: &GatewaySpec = &gateway.spec;

                // Get GatewayClass for the Gateway and match to our name of controler
                let gateway_class_res: std::result::Result<GatewayClass, KubeError> =
                    gateway_class_api
                        .get(&gateway_spec.gateway_class_name)
                        .await;
                if gateway_class_res.is_err() {
                    let e = gateway_class_res.err().unwrap();
                    match &e {
                        KubeError::Api(api) => {
                            if api.code == 404 {
                                warn!(
                                    "Fetching GatewayClass {} kubernetes API error 404",
                                    &parent.name
                                );
                                continue;
                            }
                        }
                        _ => {
                            return Err(TCPRouteError::GatewayFetchFailed(
                                namespace,
                                parent.name.clone(),
                                e,
                            )
                            .into());
                        }
                    };
                } else {
                    let gateway_class = gateway_class_res.unwrap();
                    let gateway_class_spec: &GatewayClassSpec = &gateway_class.spec;
                    if gateway_class_spec.controller_name != GATEWAY_CLASS_CONTROLLER_NAME {
                        // not managed by this implementation, check the next parent ref
                        continue;
                    }

                    match Self::verify_listener(parent, &gateway) {
                        Ok(()) => {}
                        Err(e) => {
                            info!("{}", e);
                            continue;
                        }
                    }

                    supported_gateways.push(gateway)
                }
            };
        }

        // TODO: support multiple gateways
        if supported_gateways.len() > 1 {
            return Err(TCPRouteError::TooManyGatewaysFound(
                namespace,
                route_name,
                supported_gateways.len(),
            )
            .into());
        }

        Ok(supported_gateways)
    }

    fn verify_listener(parent_ref: &TCPRouteParentRefs, gateway: &Gateway) -> Result<()> {
        let gateway_spec: &GatewaySpec = &gateway.spec;
        let namespace = gateway.metadata.namespace()?;
        let gateway_name = gateway.metadata.name()?;

        let Some(parent_port) = parent_ref.port else {
            return Err(TCPRouteError::TcpRouteParentRefPortMissing(
                namespace,
                gateway_name,
                parent_ref.name.clone(),
            )
            .into());
        };

        for listener in &gateway_spec.listeners {
            if listener.protocol == "TCP" && listener.port == parent_port {
                return Ok(());
            }
        }

        Err(TCPRouteError::GatewayListenerNotFound(
            namespace,
            gateway_name,
            parent_ref.name.clone(),
            parent_port,
        )
        .into())
    }

    async fn compile_tcproute_to_data_plane_backend(
        &self,
        tcp_route: &TCPRoute,
        gateway: &Gateway,
    ) -> Result<Targets> {
        let namespace = tcp_route.metadata.namespace()?;
        let route_name = tcp_route.metadata.name()?;

        // get gateway port
        let Some(parent_refs) = &tcp_route.spec.parent_refs else {
            return Err(TCPRouteError::TCPRouteNoParentRefs(namespace, route_name).into());
        };
        if parent_refs.is_empty() {
            return Err(TCPRouteError::TCPRouteNoParentRefs(namespace, route_name).into());
        }
        if parent_refs.len() > 1 {
            // TODO: support multiple parent refs
            return Err(TCPRouteError::TCPRouteTooManyParentRefs(
                namespace,
                route_name,
                parent_refs.len(),
            )
            .into());
        }

        let parent_ref: &TCPRouteParentRefs = &parent_refs[0];
        let Some(gw_port) = parent_ref.port else {
            return Err(TCPRouteError::TcpRouteParentRefPortMissing(
                namespace,
                route_name,
                parent_ref.name.clone(),
            )
            .into());
        };
        let gw_ips = get_gateway_ips(gateway)?;
        let gw_ip: Ipv4Addr = match gw_ips[0] {
            IpAddr::V4(v4) => v4,
            IpAddr::V6(_) => {
                return Err(TCPRouteError::GatewayIPv6NotSupported(namespace, route_name).into());
            }
        };

        let mut backend_targets: Vec<(Ipv4Addr, u16)> = vec![];
        let tcp_route_spec: &TCPRouteSpec = &tcp_route.spec;

        for rule in tcp_route_spec.rules.iter() {
            let Some(backend_refs) = &rule.backend_refs else {
                return Err(
                    TCPRouteError::TCPRouteRulesMissingBackendRef(namespace, route_name).into(),
                );
            };
            for backend_ref in backend_refs {
                let backend_ref_name = backend_ref.name.clone();
                let endpoints = self
                    .endpoints_from_backend_ref(tcp_route, backend_ref)
                    .await?;
                let Some(subsets) = endpoints.subsets else {
                    return Err(TCPRouteError::TCPRouteEndpointAddressNotReady(
                        namespace,
                        route_name,
                        backend_ref_name,
                    )
                    .into());
                };
                for subset in subsets.iter() {
                    let Some(ports) = &subset.ports else {
                        return Err(TCPRouteError::TCPRouteEndpointPortNotReady(
                            namespace,
                            route_name,
                            backend_ref_name,
                        )
                        .into());
                    };
                    if ports.is_empty() {
                        return Err(TCPRouteError::TCPRouteEndpointPortNotReady(
                            namespace,
                            route_name,
                            backend_ref_name,
                        )
                        .into());
                    }

                    let Some(addresses) = &subset.addresses else {
                        return Err(TCPRouteError::TCPRouteEndpointNoAddresses(
                            namespace,
                            route_name,
                            backend_ref_name,
                        )
                        .into());
                    };
                    for addr in addresses {
                        if addr.ip.is_empty() {
                            return Err(TCPRouteError::TCPRouteEndpointAddressEmpty(
                                namespace,
                                route_name,
                                backend_ref_name,
                            )
                            .into());
                        }
                        let pod_ip = match IpAddr::from_str(&addr.ip) {
                            Ok(addr) => addr,
                            Err(e) => {
                                warn!(
                                    "TCPRoute {namespace}/{route_name} backendRef {backend_ref_name} failed to parse address: {}. Error: {e}",
                                    &addr.ip
                                );
                                continue;
                            }
                        };
                        let pod_port = self.get_backend_port(tcp_route, backend_ref).await?;

                        match pod_ip {
                            IpAddr::V4(ip) => {
                                backend_targets.push((ip, pod_port));
                            }
                            IpAddr::V6(_) => { /* TODO: support IPv6 */ }
                        }
                    }
                }
            }
        }

        if backend_targets.is_empty() {
            return Err(TCPRouteError::TCPRouteNoHealthyBackends(namespace, route_name).into());
        }

        info!(
            "Targets {{ vip: {{ ip {:?}, port: {} }}, targets: {:?}}}",
            gw_ip, gw_port, backend_targets
        );
        Ok(Targets {
            vip: Some(Vip {
                ip: gw_ip.to_bits(),
                port: gw_port as u32,
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

    async fn endpoints_from_backend_ref(
        &self,
        tcp_route: &TCPRoute,
        backend_ref: &TCPRouteRulesBackendRefs,
    ) -> Result<Endpoints> {
        let namespace = &backend_ref
            .namespace
            .clone()
            .unwrap_or(tcp_route.metadata.namespace()?);

        let endpoint_api = Api::<Endpoints>::namespaced(self.k8s_client.clone(), namespace);
        endpoint_api
            .get(&backend_ref.name)
            .await
            .map_err(|e| K8sError::Client(e).into())
    }

    async fn get_backend_port(
        &self,
        tcp_route: &TCPRoute,
        backend_ref: &TCPRouteRulesBackendRefs,
    ) -> Result<u16> {
        let namespace = &backend_ref
            .namespace
            .clone()
            .unwrap_or(tcp_route.metadata.namespace()?);

        let Some(backend_ref_port) = &backend_ref.port else {
            return Err(TCPRouteError::BackendRefPortMissing(
                namespace.clone(),
                backend_ref.name.clone(),
            )
            .into());
        };

        let service_api = Api::<Service>::namespaced(self.k8s_client.clone(), namespace);
        let service = service_api
            .get(&backend_ref.name)
            .await
            .map_err(K8sError::Client)?;

        let Some(service_spec) = service.spec else {
            return Err(TCPRouteError::ServiceSpecMissing(
                namespace.clone(),
                backend_ref.name.clone(),
            )
            .into());
        };

        let Some(ports) = service_spec.ports else {
            return Err(TCPRouteError::ServicePortsMissing(
                namespace.clone(),
                backend_ref.name.clone(),
            )
            .into());
        };

        info!("service: {} ports: {ports:?}", &backend_ref.name);
        for port in ports {
            if port.port == *backend_ref_port {
                // Use NodePort as the highest priority (e.g. Service.spec.allocateLoadBalancerNodePorts=true)
                if let Some(node_port) = port.node_port {
                    return Ok(node_port as u16);
                };
                // Use TargetPort in case defined
                if let Some(target_port) = port.target_port {
                    let target_port = match &target_port {
                        IntOrString::Int(port) => *port,
                        IntOrString::String(sport) => match i32::from_str(sport.as_str()) {
                            Ok(port) => port,
                            Err(e) => {
                                return Err(TCPRouteError::ServiceTargetPortParseError(
                                    namespace.to_string(),
                                    backend_ref.name.to_string(),
                                    format!("{target_port:?}"),
                                    format!("{e:?}"),
                                )
                                .into());
                            }
                        },
                    };
                    return Ok(target_port as u16);
                }
                // Default to the port
                return Ok(port.port as u16);
            }
        }

        Err(
            TCPRouteError::BackendRefTargetPortMissing(namespace.clone(), backend_ref.name.clone())
                .into(),
        )
    }
}
