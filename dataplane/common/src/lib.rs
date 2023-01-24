#![no_std]

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
    pub backends: [Backend; 16],
    pub index: usize,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for Backend {}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct BackendKey {
    pub ip: u32,
    pub port: u32,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for BackendKey {}
