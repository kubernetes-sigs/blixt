use core::mem;

use aya_bpf::{
    bindings::TC_ACT_PIPE,
    helpers::bpf_csum_diff,
    programs::TcContext,
};
use aya_log_ebpf::info;

use crate::{
    bindings::{iphdr, icmphdr},
    utils::{csum_fold_helper, ptr_at, ETH_HDR_LEN, IP_HDR_LEN},
    BLIXT_CONNTRACK,
};

const ICMP_HDR_LEN: usize = mem::size_of::<icmphdr>();
const ICMP_PROTO_TYPE_UNREACH: u8 = 3;

pub fn handle_icmp_egress(ctx: TcContext) -> Result<i32, i64> {
    let ip_hdr: *mut iphdr = unsafe { ptr_at(&ctx, ETH_HDR_LEN) }?;

    let icmp_header_offset = ETH_HDR_LEN + IP_HDR_LEN;

    let icmp_hdr: *mut icmphdr = unsafe { 
        ptr_at(
            &ctx,
            icmp_header_offset
        )?
    };

    // We only care about redirecting port unreachable messages currently so a 
    // UDP client can tell when the server is shutdown
    if unsafe { (*icmp_hdr).type_ } != ICMP_PROTO_TYPE_UNREACH { 
        return Ok(TC_ACT_PIPE);
    }   

    let dest_addr = unsafe { (*ip_hdr).daddr };
    
    let new_src = unsafe { BLIXT_CONNTRACK.get(&dest_addr) }.ok_or(TC_ACT_PIPE)?;

    info!(&ctx, "Received a ICMP Unreachable packet destined for svc ip: {:ipv4} ", u32::from_be(unsafe { (*ip_hdr).daddr }));
    
    // redirect icmp unreachable message back to client
    unsafe { 
        (*ip_hdr).saddr = *new_src;
        (*ip_hdr).check = 0;
    }

    let full_cksum = unsafe { 
        bpf_csum_diff(
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            ip_hdr as *mut u32,
            mem::size_of::<iphdr>() as u32,
            0
        )
    } as u64;
    unsafe { (*ip_hdr).check = csum_fold_helper(full_cksum) };   

    // Get inner ipheader since we need to update that as well
    let icmp_inner_ip_hdr: *mut iphdr  = unsafe { ptr_at(&ctx, icmp_header_offset + ICMP_HDR_LEN)}?;

    unsafe { 
        (*icmp_inner_ip_hdr).daddr = *new_src;
        (*icmp_inner_ip_hdr).check = 0;
    }

    let full_cksum = unsafe { 
        bpf_csum_diff(
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            icmp_inner_ip_hdr as *mut u32,
            mem::size_of::<iphdr>() as u32,
            0
        )
    } as u64;
    unsafe { (*icmp_inner_ip_hdr).check = csum_fold_helper(full_cksum) };

    unsafe { BLIXT_CONNTRACK.remove(&dest_addr)? };

    return Ok(TC_ACT_PIPE);
}
