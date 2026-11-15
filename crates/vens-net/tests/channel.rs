//! The exchange follows sections 4, 7.1, and 7.2 of the ix1300-wifi protocol document: a greeting, then framed requests and replies.

use std::collections::VecDeque;

use vens_core::{
    FrameError, Ipv4Address, MacAddress,
    command::{ReplyHeaderError, ScsiStatus},
    control::GreetingError,
    scan::Command,
};
use vens_net::channel::{ChannelError, CommandChannel, Stream};

const HOST: Ipv4Address = Ipv4Address::new([192, 0, 2, 10]);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);

#[derive(Debug, PartialEq, Eq)]
struct Closed;

struct Wire {
    incoming: VecDeque<u8>,
    outgoing: Vec<u8>,
}

impl Wire {
    fn new(incoming: &[&[u8]]) -> Self {
        Self {
            incoming: incoming.iter().copied().flatten().copied().collect(),
            outgoing: Vec::new(),
        }
    }
}

impl Stream for Wire {
    type Error = Closed;

    fn send(&mut self, bytes: &[u8]) -> Result<(), Closed> {
        self.outgoing.extend_from_slice(bytes);
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<(), Closed> {
        for slot in buffer {
            *slot = self.incoming.pop_front().ok_or(Closed)?;
        }
        Ok(())
    }
}

fn greeting(status: u32) -> Vec<u8> {
    [
        0x10_u32.to_be_bytes(),
        *b"VENS",
        status.to_be_bytes(),
        [0; 4],
    ]
    .concat()
}

fn header(total: u32, scsi_status: u32) -> Vec<u8> {
    let mut frame = [
        total.to_be_bytes(),
        *b"VENS",
        [0; 4],
        scsi_status.to_be_bytes(),
    ]
    .concat();
    frame.resize(40, 0);
    frame
}

#[test]
fn opening_consumes_the_greeting() {
    let wire = Wire::new(&[&greeting(0)]);
    assert!(CommandChannel::open(wire, HOST_MAC, HOST).is_ok());
}

#[test]
fn a_refused_greeting_fails_the_open() {
    let wire = Wire::new(&[&greeting(5)]);
    assert_eq!(
        CommandChannel::open(wire, HOST_MAC, HOST).err(),
        Some(ChannelError::Greeting(GreetingError::Refused { status: 5 }))
    );
}

#[test]
fn a_command_without_data_out_sends_its_header_and_returns_the_data_in() {
    let status = Command::hardware_status();
    let data = [0xA5_u8; 0x30];
    let wire = Wire::new(&[&greeting(0), &header(40 + 0x30, 0), &data]);
    let mut buffer = [0_u8; 64];
    let outcome = CommandChannel::open(wire, HOST_MAC, HOST).map(|mut channel| {
        let reply = channel
            .execute(&status, &mut buffer)
            .map(|reply| (reply.status, reply.data.to_vec()));
        (reply, channel.into_stream().outgoing)
    });
    assert_eq!(
        outcome,
        Ok((
            Ok((ScsiStatus::Good, data.to_vec())),
            status.request(HOST_MAC, HOST).encode().to_vec()
        ))
    );
}

#[test]
fn a_command_with_data_out_sends_the_payload_after_the_header() {
    let parameters = Command::scan_parameters();
    let wire = Wire::new(&[&greeting(0), &header(40, 0)]);
    let mut buffer = [0_u8; 8];
    let sent = CommandChannel::open(wire, HOST_MAC, HOST).map(|mut channel| {
        let status = channel
            .execute(&parameters, &mut buffer)
            .map(|reply| reply.status);
        (status, channel.into_stream().outgoing)
    });
    let expected = [
        parameters.request(HOST_MAC, HOST).encode().as_slice(),
        parameters.payload(),
    ]
    .concat();
    assert_eq!(sent, Ok((Ok(ScsiStatus::Good), expected)));
}

#[test]
fn a_check_condition_still_returns_the_data_that_came_with_it() {
    let wire = Wire::new(&[&greeting(0), &header(40 + 2, 2), &[7, 9]]);
    let mut buffer = [0_u8; 8];
    let reply = CommandChannel::open(wire, HOST_MAC, HOST).map(|mut channel| {
        channel
            .execute(&Command::next_sheet(), &mut buffer)
            .map(|reply| (reply.status, reply.data.to_vec()))
    });
    assert_eq!(reply, Ok(Ok((ScsiStatus::CheckCondition, [7, 9].to_vec()))));
}

#[test]
fn a_reply_larger_than_the_buffer_is_refused() {
    let wire = Wire::new(&[&greeting(0), &header(40 + 9, 0), &[0; 9]]);
    let mut buffer = [0_u8; 8];
    let reply = CommandChannel::open(wire, HOST_MAC, HOST).map(|mut channel| {
        channel
            .execute(&Command::next_sheet(), &mut buffer)
            .map(|reply| reply.status)
    });
    assert_eq!(
        reply,
        Ok(Err(ChannelError::ReplyTooLarge {
            declared: 9,
            capacity: 8,
        }))
    );
}

#[test]
fn a_reply_header_with_a_foreign_key_fails_the_command() {
    let mut foreign = header(40, 0);
    foreign
        .iter_mut()
        .skip(4)
        .zip(b"SNEV")
        .for_each(|(slot, byte)| *slot = *byte);
    let wire = Wire::new(&[&greeting(0), &foreign]);
    let mut buffer = [0_u8; 8];
    let reply = CommandChannel::open(wire, HOST_MAC, HOST).map(|mut channel| {
        channel
            .execute(&Command::next_sheet(), &mut buffer)
            .map(|reply| reply.status)
    });
    assert_eq!(
        reply,
        Ok(Err(ChannelError::Header(ReplyHeaderError::Frame(
            FrameError::ForeignKey
        ))))
    );
}

#[test]
fn a_stream_that_closes_early_reports_the_port_failure() {
    let wire = Wire::new(&[&greeting(0)]);
    let mut buffer = [0_u8; 8];
    let reply = CommandChannel::open(wire, HOST_MAC, HOST).map(|mut channel| {
        channel
            .execute(&Command::next_sheet(), &mut buffer)
            .map(|reply| reply.status)
    });
    assert_eq!(reply, Ok(Err(ChannelError::Port(Closed))));
}
