#![no_std]
#![no_main]

use aya_bpf::bindings::{TC_ACT_OK, TC_ACT_SHOT};
use aya_bpf::macros::classifier;
use aya_bpf::programs::TcContext;
use aya_log_ebpf::info;

#[classifier(name = "tc_ingress")]
pub fn tc_ingress(ctx: TcContext) -> i32 {
    match try_tc_ingress(ctx) {
        Ok(ret) => ret,
        Err(_) => TC_ACT_SHOT,
    }
}

fn try_tc_ingress(ctx: TcContext) -> Result<i32, i64> {
    info!(&ctx, "received a packet");
    Ok(TC_ACT_OK)
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
