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

use std::net::Ipv4Addr;

use crate::Error;
use crate::consts::GATEWAY_CLASS_CONTROLLER_NAME;
use crate::traits::HasConditions;
use api_server::backends::{Target, Targets, Vip};

use gateway_api::apis::experimental::tcproutes::{TCPRouteParentRefs, TCPRouteRulesBackendRefs};
use gateway_api::apis::standard::gateways::Gateway;
use k8s_openapi::api::core::v1::Endpoints;
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;
use kube::{Api, Client};

#[allow(dead_code)]
pub async fn is_route_managed(
    client: Client,
    route_namespace: &str,
    parent_refs: &[TCPRouteParentRefs],
) -> Result<Option<Gateway>, Error> {
    for parent_ref in parent_refs {
        let gateway_namespace = parent_ref.namespace.as_deref().unwrap_or(route_namespace);
        let gateway_name = parent_ref.name.as_str();

        let gateway_api: Api<Gateway> = Api::namespaced(client.clone(), gateway_namespace);

        let gateway = match gateway_api.get(gateway_name).await {
            Ok(gw) => gw,
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => continue,
            Err(e) => return Err(Error::KubeError(e)),
        };

        let gatewayclass_api: Api<gateway_api::apis::standard::gatewayclasses::GatewayClass> =
            Api::all(client.clone());

        let gatewayclass = match gatewayclass_api.get(&gateway.spec.gateway_class_name).await {
            Ok(gwc) => gwc,
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => continue,
            Err(e) => return Err(Error::KubeError(e)),
        };

        if gatewayclass.spec.controller_name != GATEWAY_CLASS_CONTROLLER_NAME {
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

        return Ok(Some(gateway));
    }

    Ok(None)
}

#[allow(dead_code)]
pub async fn compile_route_to_targets(
    client: Client,
    route_namespace: &str,
    backend_refs: &[TCPRouteRulesBackendRefs],
    gateway: &Gateway,
    parent_refs: &[TCPRouteParentRefs],
) -> Result<Targets, Error> {
    let gateway_ip = get_gateway_ip(gateway)?;
    let gateway_port = get_gateway_port_for_refs(gateway, parent_refs)?;

    let vip = Vip {
        ip: u32::from(gateway_ip),
        port: gateway_port as u32,
    };

    let mut targets = Vec::new();

    for backend_ref in backend_refs {
        let backend_namespace = backend_ref.namespace.as_deref().unwrap_or(route_namespace);
        let backend_name = backend_ref.name.as_str();
        let backend_port = backend_ref.port.unwrap_or(80);

        let endpoints_api: Api<Endpoints> = Api::namespaced(client.clone(), backend_namespace);
        let endpoints = endpoints_api
            .get(backend_name)
            .await
            .map_err(Error::KubeError)?;

        for subset in endpoints.subsets.unwrap_or_default() {
            for address in subset.addresses.unwrap_or_default() {
                if let Ok(ip) = address.ip.parse::<Ipv4Addr>() {
                    targets.push(Target {
                        daddr: u32::from(ip),
                        dport: backend_port as u32,
                        ifindex: None,
                    });
                }
            }
        }
    }

    if targets.is_empty() {
        return Err(Error::LoadBalancerError(
            "No ready endpoints found".to_string(),
        ));
    }

    Ok(Targets {
        vip: Some(vip),
        targets,
    })
}

fn get_gateway_ip(gateway: &Gateway) -> Result<Ipv4Addr, Error> {
    let gateway_status = gateway
        .status
        .as_ref()
        .ok_or_else(|| Error::InvalidConfigError("Gateway Status Not Ready".to_string()))?;

    let gateway_address = gateway_status
        .addresses
        .as_ref()
        .and_then(|addresses| {
            if addresses.len() == 1 {
                addresses.first().and_then(|addr| addr.value.parse().ok())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Error::InvalidConfigError("Gateway must have exactly one address".to_string())
        })?;

    Ok(gateway_address)
}

pub fn get_gateway_port_for_refs(
    gateway: &Gateway,
    parent_refs: &[TCPRouteParentRefs],
) -> Result<i32, Error> {
    for parent_ref in parent_refs {
        if let Some(port) = parent_ref.port {
            for listener in &gateway.spec.listeners {
                if listener.port == port {
                    return Ok(port);
                }
            }
        }
    }

    Err(Error::InvalidConfigError(
        "No matching gateway port found".to_string(),
    ))
}

// Sets the provided condition on any Gateway API object so log as it implements
// the HasConditions trait.
//
// The condition on the object is only updated
// if the new condition has a different status (except for the observed generation which is always
// updated).
pub fn set_condition<T: HasConditions>(obj: &mut T, new_cond: metav1::Condition) {
    if let Some(conditions) = obj.get_conditions_mut() {
        for condition in conditions.iter_mut() {
            if condition.type_ == new_cond.type_ {
                if condition.status == new_cond.status {
                    // always update the observed generation
                    condition.observed_generation = new_cond.observed_generation;
                    return;
                }
                *condition = new_cond;
                return;
            }
        }
        conditions.push(new_cond);
    } else {
        obj.get_conditions_mut().replace(vec![new_cond]);
    }
}
