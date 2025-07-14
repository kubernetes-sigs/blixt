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

use api_server::backends::{Target, Targets, Vip};
use gateway_api::apis::experimental::tcproutes::{TCPRouteParentRefs, TCPRouteRulesBackendRefs};
use gateway_api::apis::standard::gateways::Gateway;
use k8s_openapi::api::core::v1::Endpoints;
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;
use kube::{Api, Client};

use crate::controllers::{GatewayError, NamespaceName, TCPRouteError};
use crate::gateway_utils::get_gateway_ipv4;
use crate::traits::HasConditions;
use crate::{Error, K8sError, NamespacedName, controllers};

// FIXME: use & integrate within controller
pub async fn compile_route_to_targets(
    client: Client,
    route_namespace: &str,
    tcp_route_key: &NamespacedName,
    backend_refs: &[TCPRouteRulesBackendRefs],
    gateway: &Gateway,
    parent_refs: &[TCPRouteParentRefs],
) -> Result<Targets, Error> {
    let gateway_ip = get_gateway_ipv4(gateway)?;
    let gateway_port = controllers::tcproute::TCPRouteController::get_gateway_port_for_parent_refs(
        tcp_route_key,
        parent_refs,
        gateway,
    )?;

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
            .map_err(K8sError::Client)?;

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

    let gw_name = gateway.metadata.name()?;
    let namespace = gateway.metadata.namespace()?;
    if targets.is_empty() {
        // FIXME: likely suited as TCPRouteError BackendRefs
        return Err(
            GatewayError::ServiceMissingLoadBalancerEndpointsReady(namespace, gw_name).into(),
        );
    }

    Ok(Targets {
        vip: Some(vip),
        targets,
    })
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
