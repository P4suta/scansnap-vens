//! The fixture network offers the recorded scanner at the endpoints of its recorded search reply and nowhere else.

use std::net::{Ipv4Addr, SocketAddrV4};

use vens_core::{Ipv4Address, Port, control::LocalTime};
use vens_emulator::{
    network::{Contact, FixtureNetwork},
    redact::{HOST_ADDRESS, SCANNER_ADDRESS, TRIGGER_PORT},
    replay::ReplayError,
};
use vens_net::{
    channel::Stream,
    network::{Connection, Network},
    search::Datagram,
    trace,
};

const FIXTURE: &str = include_str!("../traces/three-duplex-sheets.trace");

fn network() -> Option<FixtureNetwork> {
    FixtureNetwork::new(trace::parse(FIXTURE).ok()?)
}

fn endpoint(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::from(SCANNER_ADDRESS), port)
}

fn greeting(
    network: &mut FixtureNetwork,
    port: u16,
    connection: Connection,
) -> Result<[u8; 16], ReplayError> {
    let mut stream = network.connect(endpoint(port), connection)?;
    let mut greeting = [0; 16];
    stream.receive(&mut greeting)?;
    Ok(greeting)
}

#[test]
fn the_recorded_endpoints_replay_their_own_channels() {
    let expected = [
        0x00, 0x00, 0x00, 0x10, 0x56, 0x45, 0x4E, 0x53, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(
        network().map(|mut network| (
            greeting(&mut network, 53218, Connection::Command),
            greeting(&mut network, 53219, Connection::Control),
        )),
        Some((Ok(expected), Ok(expected)))
    );
}

#[test]
fn any_other_endpoint_is_refused() {
    let other_address = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 8), 53218);
    assert_eq!(
        network().map(|mut network| (
            network.connect(endpoint(53220), Connection::Command).err(),
            network.connect(endpoint(53219), Connection::Command).err(),
            network.connect(endpoint(53218), Connection::Control).err(),
            network.connect(other_address, Connection::Command).err(),
        )),
        Some((
            Some(ReplayError::UnknownEndpoint),
            Some(ReplayError::UnknownEndpoint),
            Some(ReplayError::UnknownEndpoint),
            Some(ReplayError::UnknownEndpoint),
        ))
    );
}

#[test]
fn the_host_facilities_carry_the_documentation_values() {
    assert_eq!(
        network().map(|mut network| (
            network.local_address(Ipv4Addr::BROADCAST),
            network
                .open_trigger(Ipv4Address::new(HOST_ADDRESS))
                .map(|((), port)| port),
            network.local_time(),
        )),
        Some((
            Ok(Ipv4Address::new(HOST_ADDRESS)),
            Ok(Port::new(TRIGGER_PORT)),
            LocalTime::new(2026, 1, 1, 0, 0, 0).ok_or(ReplayError::Exhausted),
        ))
    );
}

#[test]
fn a_fixture_without_a_search_reply_gives_no_network() {
    assert!(FixtureNetwork::new(Vec::new()).is_none());
}

#[test]
fn an_observed_network_reports_every_datagram_target_and_connection_in_order() {
    let observed = trace::parse(FIXTURE)
        .ok()
        .and_then(FixtureNetwork::observed)
        .map(|(mut network, contacts)| {
            let probe_target = SocketAddrV4::new(Ipv4Addr::BROADCAST, 52217);
            let sent = network
                .open_discovery()
                .and_then(|mut discovery| discovery.send(&[0x56], probe_target));
            let refused = network.connect(endpoint(53220), Connection::Command).err();
            let accepted = network
                .connect(endpoint(53218), Connection::Command)
                .is_ok();
            (
                sent.is_err(),
                refused,
                accepted,
                contacts.try_iter().collect::<Vec<_>>(),
            )
        });
    assert_eq!(
        observed,
        Some((
            true,
            Some(ReplayError::UnknownEndpoint),
            true,
            [
                Contact::Datagram(SocketAddrV4::new(Ipv4Addr::BROADCAST, 52217)),
                Contact::Connect(endpoint(53220), Connection::Command),
                Contact::Connect(endpoint(53218), Connection::Command),
            ]
            .to_vec()
        ))
    );
}
