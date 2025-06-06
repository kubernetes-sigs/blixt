/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

#![allow(static_mut_refs)]

use core::mem;

use aya_ebpf::{
    bindings::{TC_ACT_OK, TC_ACT_PIPE},
    helpers::bpf_csum_diff,
    programs::TcContext,
};
use aya_log_ebpf::info;
use common::ClientKey;
use network_types::{eth::EthHdr, ip::Ipv4Hdr, tcp::TcpHdr};

use crate::{
    utils::{csum_fold_helper, ptr_at, update_tcp_conns},
    LB_CONNECTIONS,
};

pub fn handle_tcp_egress(ctx: TcContext) -> Result<i32, i64> {
    // gather the TCP header
    let ip_hdr: *mut Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };

    let tcp_header_offset = EthHdr::LEN + Ipv4Hdr::LEN;

    let tcp_hdr: *mut TcpHdr = unsafe { ptr_at(&ctx, tcp_header_offset)? };

    // capture some IP and port information
    let client_addr = unsafe { (*ip_hdr).dst_addr };
    let dest_port = unsafe { (*tcp_hdr).dest };
    // The source identifier
    let client_key = ClientKey {
        ip: u32::from_be(client_addr),
        port: u16::from_be(dest_port) as u32,
    };
    let lb_mapping = unsafe { LB_CONNECTIONS.get(&client_key) }.ok_or(TC_ACT_PIPE)?;

    info!(
        &ctx,
        "Received TCP packet destined for tracked IP {:i}:{} setting source IP to VIP {:i}:{}",
        u32::from_be(client_addr),
        u16::from_be(dest_port),
        lb_mapping.backend_key.ip,
        lb_mapping.backend_key.port,
    );

    // TODO: connection tracking cleanup https://github.com/kubernetes-sigs/blixt/issues/85
    // SNAT the ip address
    unsafe {
        (*ip_hdr).src_addr = lb_mapping.backend_key.ip.to_be();
    };
    // SNAT the port
    unsafe { (*tcp_hdr).source = u16::from_be(lb_mapping.backend_key.port as u16) };

    if (ctx.data() + EthHdr::LEN + Ipv4Hdr::LEN) > ctx.data_end() {
        info!(&ctx, "Iphdr is out of bounds");
        return Ok(TC_ACT_OK);
    }

    unsafe { (*ip_hdr).check = 0 };
    let full_cksum = unsafe {
        bpf_csum_diff(
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            ip_hdr as *mut u32,
            Ipv4Hdr::LEN as u32,
            0,
        )
    } as u64;
    unsafe { (*ip_hdr).check = csum_fold_helper(full_cksum) };
    unsafe { (*tcp_hdr).check = 0 };

    let tcp_hdr_ref = unsafe { tcp_hdr.as_ref().ok_or(TC_ACT_OK)? };

    // If the packet has the RST flag set, it means the connection is being terminated, so remove it
    // from our map.
    if tcp_hdr_ref.rst() == 1 {
        unsafe {
            LB_CONNECTIONS.remove(&client_key)?;
        }
    }

    let mut mapping = *lb_mapping;
    update_tcp_conns(tcp_hdr_ref, &client_key, &mut mapping)?;

    Ok(TC_ACT_PIPE)
}
