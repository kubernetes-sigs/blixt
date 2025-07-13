pub mod tcproute;

use gateway_api::apis::standard::gateways::Gateway;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::net::IpAddr;
use std::str::FromStr;
use tracing::warn;

use crate::controllers::tcproute::TCPRouteError;
use crate::{Error, Result};

// FIXME: potentially drop pub after moving files
pub(crate) trait NamespaceName {
    fn namespace(&self) -> Result<String>;
    fn name(&self) -> Result<String>;
}

impl NamespaceName for ObjectMeta {
    fn namespace(&self) -> Result<String> {
        self.namespace
            .clone()
            .ok_or(Error::InvalidConfigError("missing namespace".to_string()))
    }

    fn name(&self) -> Result<String> {
        self.name
            .clone()
            .ok_or(Error::InvalidConfigError("missing name".to_string()))
    }
}

fn get_gateway_ips(gateway: &Gateway) -> Result<Vec<IpAddr>> {
    let namespace = gateway.metadata.namespace()?;
    let gw_name = gateway.metadata.name()?;

    let Some(status) = &gateway.status else {
        return Err(TCPRouteError::GatewayNoStatus(namespace, gw_name).into());
    };

    let Some(addresses) = &status.addresses else {
        return Err(TCPRouteError::GatewayNoIpAddress(namespace, gw_name).into());
    };
    if addresses.len() != 1 {
        return Err(TCPRouteError::OnlySingleGatewayIpAddressSupported(
            namespace,
            gw_name,
            addresses.len(),
        )
        .into());
    }

    let ip_addresses = addresses.iter()
        .filter(|a| {
            if let Some(r#type) = &a.r#type {
                r#type == "IPAddress"
            } else {
                false
            }
        })
        .filter_map(|a| {
            match IpAddr::from_str(&a.value) {
                Ok(addr) => if addr.is_ipv4() {
                    Some(addr)
                } else {
                    warn!("Gateway IpAddress {}. IPv6 addresses are currently not supported. Skipping.", a.value);
                    None
                }
                Err(e) => {
                    warn!("Failed to parse Gateway IpAddress {}. Error: {}. Skipping.", a.value, e);
                    None
                }
            }
        })
        .collect::<Vec<IpAddr>>();

    if ip_addresses.is_empty() {
        return Err(TCPRouteError::GatewayNoIPv4Address(namespace, gw_name).into());
    };

    Ok(ip_addresses)
}
