use std::net::{self, SocketAddr};
use std::str::FromStr;

use anyhow::Error;
use clap::Parser;
use tonic::Request;

use api_server::backends::backends_client::BackendsClient;
use api_server::backends::{Target, Targets, Vip};

#[derive(Debug, Parser)]
pub struct Options {
    #[clap(default_value = "127.0.0.1", long)]
    pub server_ip: String,
    #[clap(default_value = "9874", long)]
    pub server_port: u32,
    #[clap(default_value = "127.0.0.1", long)]
    pub vip_ip: String,
    #[clap(default_value = "8080", long)]
    pub vip_port: u32,
    #[clap(default_value = "127.0.0.1", long)]
    pub daddr: String,
    #[clap(default_value = "8080", long)]
    pub dport: u32,
    #[clap(default_value = "0", long)]
    pub ifindex: u32,
    #[clap(long, short, action)]
    pub delete: bool,
}

pub async fn update(opts: Options) -> Result<(), Error> {
    let server_addr: SocketAddr = format!("{}:{}", opts.server_ip, opts.server_port).parse()?;

    let mut client = BackendsClient::connect(format!("http://{}", server_addr)).await?;

    let addr = net::Ipv4Addr::from_str(&opts.vip_ip)?;
    let daddr = net::Ipv4Addr::from_str(&opts.daddr)?;

    let vip = Vip {
        ip: addr.into(),
        port: opts.vip_port,
    };

    let req = Request::new(Targets {
        vip: Some(vip.clone()),
        target: Some(Target {
            daddr: daddr.into(),
            dport: opts.dport,
            ifindex: Some(opts.ifindex),
        }),
    });

    if opts.delete {
        let res = client.delete(vip.clone()).await?;
        println!(
            "grpc server responded to DELETE: {}",
            res.into_inner().confirmation
        );
    } else {
        let res = client.update(req).await?;
        println!(
            "grpc server responded to UPDATE: {}",
            res.into_inner().confirmation
        );
    }

    Ok(())
}
