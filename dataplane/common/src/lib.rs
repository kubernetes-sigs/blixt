#![no_std]

pub const BACKENDS_ARRAY_LENGTH: usize = 16;

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Backend {
    pub daddr: u32,
    pub dport: u32,
    pub ifindex: u16,
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct BackendsList {
    pub backends: [Backend; BACKENDS_ARRAY_LENGTH],
    pub index: usize,
    pub n_elements: usize,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for BackendsList {}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct BackendKey {
    pub ip: u32,
    pub port: u32,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for BackendKey {}
