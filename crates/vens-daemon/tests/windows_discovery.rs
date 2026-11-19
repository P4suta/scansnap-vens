#![cfg(windows)]

use std::{
    env,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use vens_escl::uuid::Uuid;

const SERVICE: &str = "_uscan._tcp.local.";
const SEARCH: Duration = Duration::from_secs(12);
const HTTP: Duration = Duration::from_secs(2);

fn read_capabilities(address: SocketAddr, path: &str, uuid: &str) -> io::Result<()> {
    let mut socket = TcpStream::connect_timeout(&address, HTTP)?;
    socket.set_read_timeout(Some(HTTP))?;
    socket.set_write_timeout(Some(HTTP))?;
    write!(
        socket,
        "GET /{path}/ScannerCapabilities HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    socket.take(16_384).read_to_string(&mut response)?;
    if response.starts_with("HTTP/1.1 200 ")
        && response.contains("ScannerCapabilities")
        && response.contains(uuid)
    {
        return Ok(());
    }
    Err(io::Error::other(
        "the discovered endpoint returned different capabilities",
    ))
}

fn discover(browser: &ServiceDaemon, port: u16, uuid: &str) -> io::Result<()> {
    let events = browser.browse(SERVICE).map_err(io::Error::other)?;
    let started = Instant::now();
    let mut last_http_error = None;
    while let Ok(event) = events.recv_timeout(SEARCH.saturating_sub(started.elapsed())) {
        if started.elapsed() >= SEARCH {
            break;
        }
        let ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };
        if service.get_port() != port || service.get_property_val_str("uuid") != Some(uuid) {
            continue;
        }
        let Some(path) = service
            .get_property_val_str("rs")
            .filter(|path| *path == "eSCL")
        else {
            return Err(io::Error::other(
                "the registration has a different eSCL path",
            ));
        };
        for address in service.get_addresses_v4() {
            match read_capabilities(SocketAddr::from((*address, port)), path, uuid) {
                Ok(()) => return Ok(()),
                Err(error) => last_http_error = Some(error),
            }
        }
    }
    Err(last_http_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "no DNS-SD record resolved to an IPv4 address for vensd",
        )
    }))
}

#[test]
fn dns_sd_clients_reach_the_windows_daemon() -> io::Result<()> {
    let port = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?
        .local_addr()?
        .port();
    let uuid = Uuid::from_bytes([0x53; 16]).to_string();
    let program =
        env::var_os("VENSD_UNDER_TEST").unwrap_or_else(|| env!("CARGO_BIN_EXE_vensd").into());
    let browser = ServiceDaemon::new().map_err(io::Error::other)?;
    let mut daemon = Command::new(program)
        .args([
            "--port",
            &port.to_string(),
            "--address",
            "192.0.2.7",
            "--uuid",
            &uuid,
        ])
        .stderr(Stdio::piped())
        .spawn()?;
    let result = discover(&browser, port, &uuid);
    let shutdown = browser.shutdown().map_err(io::Error::other);
    if daemon.try_wait()?.is_none() {
        daemon.kill()?;
    }
    let output = daemon.wait_with_output()?;
    result.map_err(|error| {
        io::Error::other(format!(
            "{error}\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
    })?;
    shutdown?.recv_timeout(HTTP).map_err(io::Error::other)?;
    Ok(())
}
