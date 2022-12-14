use std::io::{Error, ErrorKind};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UdpSocket},
    signal,
    sync::mpsc::{self, Receiver, Sender},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel(3);
    tokio::spawn(run_server(9875, tx.clone()));
    tokio::spawn(run_server(9876, tx.clone()));
    tokio::spawn(run_server(9877, tx.clone()));
    tokio::spawn(run_health_server(9878, rx));
    signal::ctrl_c().await?;
    Ok(())
}

async fn run_server(port: u16, start_notifier: Sender<u16>) -> std::io::Result<()> {
    let bindaddr = format!("0.0.0.0:{}", port);
    let sock = UdpSocket::bind(&bindaddr).await?;

    match start_notifier.send(port).await {
        Err(err) => return Err(Error::new(ErrorKind::BrokenPipe, err)),
        Ok(_) => {}
    };

    let mut buf = [0; 1024];
    loop {
        let (len, addr) = sock.recv_from(&mut buf).await?;
        println!("port {}: {} bytes received from {}", port, len, addr);
        println!(
            "port {}: buffer contents: {}",
            port,
            String::from_utf8_lossy(&buf).replace("\n", "")
        );
    }
}

async fn run_health_server(port: u16, mut rx: Receiver<u16>) -> std::io::Result<()> {
    let bindaddr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&bindaddr).await?;

    println!("waiting for listeners...");
    let mut wait_for = 3;
    while wait_for > 0 {
        match rx.recv().await {
            Some(port) => {
                println!("UDP worker listening on port {}", port);
                wait_for = wait_for - 1;
            }
            None => {}
        };
    }

    println!("health check server listening on {}", port);

    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = [0; 1024];
        stream.read(&mut buf).await?;
        stream.write_all("ready".as_bytes()).await?;
        let peer = stream.peer_addr()?;
        println!("received health check from {}", peer);
    }
}
