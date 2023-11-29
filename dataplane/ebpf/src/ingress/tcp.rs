/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use core::mem;

use aya_bpf::{
    bindings::TC_ACT_OK,
    helpers::{bpf_csum_diff, bpf_redirect_neigh},
    programs::TcContext,
};
use aya_log_ebpf::info;
use network_types::{eth::EthHdr, ip::Ipv4Hdr, tcp::TcpHdr};

use crate::{
    utils::{csum_fold_helper, ptr_at, ETH_HDR_LEN, IP_HDR_LEN},
    BACKENDS, BLIXT_CONNTRACK,
};
use common::BackendKey;

pub fn handle_tcp_ingress(ctx: TcContext) -> Result<i32, i64> {
    let ip_hdr: *mut Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };

    let tcp_hdr: *mut TcpHdr =
                unsafe { ptr_at(&ctx, EthHdr::LEN + Ipv4Hdr::LEN) }?;

    let original_daddr = unsafe { (*ip_hdr).dst_addr };

    let key = BackendKey {
        ip: u32::from_be(original_daddr),
        port: (u16::from_be(unsafe { (*tcp_hdr).dest })) as u32,
    };
    let backend_list = unsafe { BACKENDS.get(&key) }.ok_or(TC_ACT_OK)?;
    // Only a single backend is supported for TCP connections.
    // TODO(aryan9600): Add support for multiple backends (https://github.com/kubernetes-sigs/blixt/issues/119)
    let backend = backend_list.backends[0];

    info!(
        &ctx,
        "Received a TCP packet destined for svc ip: {:i} at Port: {} ",
        u32::from_be(original_daddr),
        u16::from_be(unsafe { (*tcp_hdr).dest })
    );

    unsafe {
        (*ip_hdr).dst_addr = backend.daddr.to_be();
    }

    if (ctx.data() + ETH_HDR_LEN + IP_HDR_LEN) > ctx.data_end() {
        info!(&ctx, "Iphdr is out of bounds");
        return Ok(TC_ACT_OK);
    }

    // Calculate l3 cksum
    // TODO(astoycos) use l3_cksum_replace instead
    unsafe { (*ip_hdr).check = 0 };
    let full_cksum = unsafe {
        bpf_csum_diff(
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            ip_hdr as *mut u32,
            IP_HDR_LEN as u32,
            0,
        )
    } as u64;
    unsafe { (*ip_hdr).check = csum_fold_helper(full_cksum) };

    // Update destination port
    unsafe { (*tcp_hdr).dest = (backend.dport as u16).to_be() };
    // FIXME
    unsafe { (*tcp_hdr).check = 0 };

    let action = unsafe {
        bpf_redirect_neigh(
            backend.ifindex as u32,
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            0,
        )
    };

    unsafe {
        BLIXT_CONNTRACK.insert(
            &(*ip_hdr).src_addr,
            &(original_daddr, (*tcp_hdr).source.to_be() as u32),
            0 as u64,
        )?;
    };

    info!(&ctx, "redirect action: {}", action);

    Ok(action as i32)
}
