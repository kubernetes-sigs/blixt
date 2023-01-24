#![no_std]
#![no_main]

#[allow(non_upper_case_globals)]
#[allow(non_snake_case)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
mod bindings;
mod ingress;
mod utils;
mod egress;

use memoffset::offset_of;

use aya_bpf::{
    bindings::{TC_ACT_PIPE, TC_ACT_SHOT, TC_ACT_OK},
    macros::{classifier, map},
    maps::HashMap,
    programs::TcContext,
};

use bindings::{ethhdr, iphdr};
use common::{BackendKey, BackendsList, Backend};
use ingress::{tcp::handle_tcp_ingress, udp::handle_udp_ingress};
use egress::{icmp::handle_icmp_egress};
use utils::{ETH_HDR_LEN, ETH_P_IP, IPPROTO_TCP, IPPROTO_UDP, IPPROTO_ICMP};

// -----------------------------------------------------------------------------
// Maps
// -----------------------------------------------------------------------------

#[map(name = "UDP_BACKENDS")]
static mut UDP_BACKENDS: HashMap<BackendKey, BackendsList> =
    HashMap::<BackendKey, BackendsList>::with_max_entries(128, 0);

#[map(name = "TCP_BACKENDS")]
static mut TCP_BACKENDS: HashMap<BackendKey, Backend> =
    HashMap::<BackendKey, Backend>::with_max_entries(128, 0);

#[map(name = "BLIXT_CONNTRACK")] 
static mut BLIXT_CONNTRACK: HashMap<u32, u32> =
    HashMap::<u32, u32>::with_max_entries(128, 0);

// -----------------------------------------------------------------------------
// Ingress
// -----------------------------------------------------------------------------

#[classifier(name = "tc_ingress")]
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
    let h_proto = u16::from_be(
        ctx.load(offset_of!(ethhdr, h_proto))
            .map_err(|_| TC_ACT_PIPE)?,
    );

    if h_proto != ETH_P_IP {
        return Ok(TC_ACT_PIPE);
    }

    let protocol = ctx
        .load::<u8>(ETH_HDR_LEN + offset_of!(iphdr, protocol))
        .map_err(|_| TC_ACT_PIPE)?;

    match protocol {
        IPPROTO_TCP => handle_tcp_ingress(ctx),
        IPPROTO_UDP => handle_udp_ingress(ctx),
        _ => Ok(TC_ACT_PIPE),
    }
}

// -----------------------------------------------------------------------------
// Egress
// -----------------------------------------------------------------------------

#[classifier(name = "tc_egress")]
pub fn tc_egress(ctx: TcContext) -> i32 {
    match try_tc_egress(ctx) {
        Ok(ret) => ret,
        Err(_) => TC_ACT_SHOT,
    };

    // TODO(https://github.com/Kong/blixt/issues/69) better Error reporting framework
    return TC_ACT_OK;
}

fn try_tc_egress(ctx: TcContext) -> Result<i32, i64> {
    let h_proto = u16::from_be(
        ctx.load(offset_of!(ethhdr, h_proto))
            .map_err(|_| TC_ACT_PIPE)?,
    );

    if h_proto != ETH_P_IP {
        return Ok(TC_ACT_PIPE);
    }

    let protocol = ctx
        .load::<u8>(ETH_HDR_LEN + offset_of!(iphdr, protocol))
        .map_err(|_| TC_ACT_PIPE)?;

    match protocol {
        IPPROTO_ICMP => handle_icmp_egress(ctx),
        _ => Ok(TC_ACT_PIPE),
    }
}

// -----------------------------------------------------------------------------
// Panic Implementation
// -----------------------------------------------------------------------------

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
