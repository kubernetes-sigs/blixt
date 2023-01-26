use core::mem;

use aya_bpf::{
    bindings::TC_ACT_OK,
    helpers::{bpf_csum_diff, bpf_redirect_neigh},
    programs::TcContext,
};
use aya_log_ebpf::info;

use crate::{
    bindings::{iphdr, tcphdr},
    utils::{csum_fold_helper, ip_from_int, ptr_at, ETH_HDR_LEN, IP_HDR_LEN},
    BACKENDS,
};
use common::BackendKey;

pub fn handle_tcp_ingress(ctx: TcContext) -> Result<i32, i64> {
    let ip_hdr: *mut iphdr = unsafe { ptr_at(&ctx, ETH_HDR_LEN) }?;

    let tcp_header_offset = ETH_HDR_LEN + IP_HDR_LEN;

    let tcp_hdr: *mut tcphdr = unsafe { ptr_at(&ctx, tcp_header_offset)? };

    let daddr_dot_dec = ip_from_int(unsafe { (*ip_hdr).daddr });
    info!(
        &ctx,
        "Received a TCP packet destined for svc ip: {}.{}.{}.{} at port: {}",
        daddr_dot_dec[0],
        daddr_dot_dec[1],
        daddr_dot_dec[2],
        daddr_dot_dec[3],
        u16::from_be(unsafe { (*tcp_hdr).dest })
    );

    let key = BackendKey {
        ip: u32::from_be(unsafe { (*ip_hdr).daddr }),
        port: (u16::from_be(unsafe { (*tcp_hdr).dest })) as u32,
    };

    let backends_list = unsafe { BACKENDS.get(&key) }.ok_or(TC_ACT_OK)?;
    if backends_list.n_elements == 0 {
        return Ok(TC_ACT_OK)
    }
    let backend = backends_list.backends[0];

    unsafe {
        (*ip_hdr).daddr = backend.daddr.to_be();
    }

    if (ctx.data() + ETH_HDR_LEN + mem::size_of::<iphdr>()) > ctx.data_end() {
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
            mem::size_of::<iphdr>() as u32,
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

    info!(&ctx, "redirect action: {}", action);

    Ok(action as i32)
}
