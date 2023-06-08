use core::mem;

use aya_bpf::{
    bindings::{TC_ACT_OK, TC_ACT_PIPE},
    helpers::bpf_csum_diff,
    programs::TcContext,
};
use aya_log_ebpf::info;

use crate::{
    bindings::{iphdr, tcphdr},
    utils::{csum_fold_helper, ptr_at, ETH_HDR_LEN, IP_HDR_LEN},
    BLIXT_CONNTRACK,
};

pub fn handle_tcp_egress(ctx: TcContext) -> Result<i32, i64> {
    // gather the TCP header
    let ip_hdr: *mut iphdr = unsafe { ptr_at(&ctx, ETH_HDR_LEN) }?;
    let tcp_header_offset = ETH_HDR_LEN + IP_HDR_LEN;
    let tcp_hdr: *mut tcphdr = unsafe { ptr_at(&ctx, tcp_header_offset)? };

    // capture some IP and port information
    let client_addr = unsafe { (*ip_hdr).daddr };
    let dest_port = unsafe { (*tcp_hdr).dest.to_be() };
    let ip_port_tuple = unsafe { BLIXT_CONNTRACK.get(&client_addr) }.ok_or(TC_ACT_PIPE)?;

    // verify traffic destination
    if ip_port_tuple.1 as u16 != dest_port {
        return Ok(TC_ACT_PIPE);
    }

    info!(
        &ctx,
        "Received TCP packet destined for tracked IP {:i}:{} setting source IP to VIP {:i}",
        u32::from_be(client_addr),
        ip_port_tuple.1 as u16,
        u32::from_be(ip_port_tuple.0),
    );

    unsafe {
        (*ip_hdr).saddr = ip_port_tuple.0;
    };

    if (ctx.data() + ETH_HDR_LEN + mem::size_of::<iphdr>()) > ctx.data_end() {
        info!(&ctx, "Iphdr is out of bounds");
        return Ok(TC_ACT_OK);
    }

    unsafe { (*ip_hdr).check = 0 };
    let full_cksum = unsafe {
        bpf_csum_diff(
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            ip_hdr as *mut u32,
            mem::size_of::<iphdr>() as u32,
            0,
        )
    } as u64;
    unsafe { (*ip_hdr).check = csum_fold_helper(full_cksum) };
    unsafe { (*tcp_hdr).check = 0 };

    // TODO: connection tracking cleanup https://github.com/kong/blixt/issues/85

    Ok(TC_ACT_PIPE)
}
