/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

#![no_std]
#![no_main]

#[allow(non_upper_case_globals)]
#[allow(non_snake_case)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
mod egress;
mod ingress;
mod utils;

use aya_ebpf::{
    bindings::{TC_ACT_OK, TC_ACT_PIPE, TC_ACT_SHOT},
    macros::{classifier, map},
    maps::HashMap,
    programs::TcContext,
};

use common::{BackendKey, BackendList, ClientKey, LoadBalancerMapping, BPF_MAPS_CAPACITY};
use egress::{icmp::handle_icmp_egress, tcp::handle_tcp_egress};
use ingress::{tcp::handle_tcp_ingress, udp::handle_udp_ingress};

use network_types::{
    eth::{EthHdr, EtherType},
    ip::{IpProto, Ipv4Hdr},
};
use utils::ptr_at;

// -----------------------------------------------------------------------------
// Maps
// -----------------------------------------------------------------------------

#[map(name = "BACKENDS")]
static mut BACKENDS: HashMap<BackendKey, BackendList> =
    HashMap::<BackendKey, BackendList>::with_max_entries(BPF_MAPS_CAPACITY, 0);

#[map(name = "GATEWAY_INDEXES")]
static mut GATEWAY_INDEXES: HashMap<BackendKey, u16> =
    HashMap::<BackendKey, u16>::with_max_entries(BPF_MAPS_CAPACITY, 0);

#[map(name = "LB_CONNECTIONS")]
static mut LB_CONNECTIONS: HashMap<ClientKey, LoadBalancerMapping> =
    HashMap::<ClientKey, LoadBalancerMapping>::with_max_entries(128, 0);

// -----------------------------------------------------------------------------
// Ingress
// -----------------------------------------------------------------------------

#[classifier]
pub fn tc_ingress(ctx: TcContext) -> i32 {
    match try_tc_ingress(ctx) {
        Ok(ret) => ret,
        Err(_) => TC_ACT_SHOT,
    };

    // TODO(https://github.com/Kong/blixt/issues/69) better Error reporting framework
    return TC_ACT_OK;
}

// Make sure ip_forwarding is enabled on the interface this it attached to
fn try_tc_ingress(ctx: TcContext) -> Result<i32, i64> {
    let eth_hdr: *const EthHdr = unsafe { ptr_at(&ctx, 0) }?;
    match unsafe { *eth_hdr }.ether_type {
        EtherType::Ipv4 => {
            let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };
            match unsafe { *ipv4hdr }.proto {
                IpProto::Tcp => handle_tcp_ingress(ctx),
                IpProto::Udp => handle_udp_ingress(ctx),
                _ => Ok(TC_ACT_PIPE),
            }
        }
        _ => return Ok(TC_ACT_PIPE),
    }
}

// -----------------------------------------------------------------------------
// Egress
// -----------------------------------------------------------------------------

#[classifier]
pub fn tc_egress(ctx: TcContext) -> i32 {
    match try_tc_egress(ctx) {
        Ok(ret) => ret,
        Err(_) => TC_ACT_SHOT,
    };

    // TODO(https://github.com/Kong/blixt/issues/69) better Error reporting framework
    return TC_ACT_OK;
}

fn try_tc_egress(ctx: TcContext) -> Result<i32, i64> {
    let eth_hdr: *const EthHdr = unsafe { ptr_at(&ctx, 0) }?;
    match unsafe { *eth_hdr }.ether_type {
        EtherType::Ipv4 => {
            let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };
            match unsafe { *ipv4hdr }.proto {
                IpProto::Icmp => handle_icmp_egress(ctx),
                IpProto::Tcp => handle_tcp_egress(ctx),
                _ => Ok(TC_ACT_PIPE),
            }
        }
        _ => return Ok(TC_ACT_PIPE),
    }
}

// -----------------------------------------------------------------------------
// Panic Implementation
// -----------------------------------------------------------------------------

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
