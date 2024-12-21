/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

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

    // Store the old source IP address for checksum calculation
    let old_src_addr = unsafe { (*ip_hdr).src_addr };

    // Store the old source port for checksum calculation
    let old_src_port = unsafe { (*tcp_hdr).source };

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

    let ret = unsafe {
        bpf_l3_csum_replace(
            ctx.skb.skb,
            Ipv4Addr::Len as u32,
            old_src_addr as u64,
            lb_mapping.backend_key.ip.to_be() as u64,
            4,
        )
    };

    if ret != 0 {
        info!(&ctx, "Failed to update IP checksum");
        return Ok(TC_ACT_OK);
    }

    let ret = unsafe {
        bpf_l4_csum_replace(
            ctx.skb.skb,
            tcp_header_offset as u32,
            old_src_port as u64,
            lb_mapping.backend_key.port.to_be() as u64,
            2, 
        )
    };

    if ret != 0 {
        info!(&ctx, "Failed to update TCP checksum");
        return Ok(TC_ACT_OK);
    }

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
