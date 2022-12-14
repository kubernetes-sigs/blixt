use anyhow::Error;
use libc::if_nametoindex as libc_if_nametoindex;
use regex::Regex;
use std::ffi::CString;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use std::str::from_utf8;

/// Returns an ifindex for a provided ifname. Wraps libc.
pub fn if_nametoindex(ifname: String) -> Result<u32, Error> {
    let ifname_c = CString::new(ifname)?;
    let ifindex = unsafe { libc_if_nametoindex(ifname_c.into_raw()) };
    Ok(ifindex)
}

/// Given an IPv4 address will return the local system's network interface
/// which is responsible for routing that address. Not portable: only works on
/// Linux systems with iproute2 installed.
///
/// TODO: replace this https://github.com/Kong/blixt/issues/49
pub fn if_name_for_routing_ip(ip_addr: Ipv4Addr) -> Result<String, Error> {
    // run the linux command "ip route" to get the device responsible for
    // routing the given IP address.
    let ip = ip_addr.to_string();
    let mut cmd = Command::new("ip");
    let child = cmd
        .arg("route")
        .arg("get")
        .arg("to")
        .arg(&ip)
        .stdout(Stdio::piped())
        .spawn()?;

    // grab the output from the "ip route" command
    let output = child.wait_with_output()?;
    let stdout = from_utf8(output.stdout.as_slice())?;

    // construct a regex to match the output
    let mut regex_str = String::from(ip.clone());
    regex_str.push_str(r" (via [0-9]+\.[0-9]+\.[0-9]+\.[0-9]+ )?dev ([a-zA-Z0-9]+)\s+");
    let re = Regex::new(&regex_str)?;

    // match on the output to find the network device responsible for routing
    // the provided IP address.
    let match_err = format!("no device found to route {}", ip);
    let device = re
        .captures(stdout)
        .ok_or(Error::msg(match_err.clone()))?
        .iter()
        .last()
        .ok_or(Error::msg(match_err.clone()))?
        .ok_or(Error::msg(match_err.clone()))?
        .as_str()
        .to_owned();

    Ok(device)
}
