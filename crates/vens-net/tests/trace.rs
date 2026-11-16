//! A trace keeps every frame that the host sends or receives, one event per line, so that a scan can be replayed and reviewed.

use vens_net::trace::{Channel, Direction, Event, Fault, TraceError, parse, render};

fn event(channel: Channel, direction: Direction, bytes: &[u8]) -> Event {
    Event {
        channel,
        direction,
        bytes: bytes.to_vec(),
    }
}

fn sample() -> Vec<Event> {
    [
        event(Channel::Discovery, Direction::Sent, &[0x56, 0x45]),
        event(Channel::Command, Direction::Received, &[0x00, 0xFF, 0x10]),
        event(Channel::Control, Direction::Sent, &[0xAB]),
        event(Channel::Command, Direction::Sent, &[0x0A, 0x0B]),
    ]
    .to_vec()
}

#[test]
fn each_event_takes_one_line_with_its_channel_direction_and_hex_bytes() {
    assert_eq!(
        render(&sample()),
        "discovery > 5645\ncommand < 00ff10\ncontrol > ab\ncommand > 0a0b\n"
    );
}

#[test]
fn a_rendered_trace_parses_back_to_the_same_events() {
    assert_eq!(parse(&render(&sample())), Ok(sample()));
}

#[test]
fn hex_digits_in_either_case_are_accepted() {
    assert_eq!(
        parse("control < aBcD\n"),
        Ok([event(Channel::Control, Direction::Received, &[0xAB, 0xCD])].to_vec())
    );
}

#[test]
fn an_unknown_channel_is_refused_with_its_line_number() {
    assert_eq!(
        parse("command > 00\nscanner > 00\n"),
        Err(TraceError {
            line: 2,
            fault: Fault::UnknownChannel
        })
    );
}

#[test]
fn an_unknown_direction_is_refused() {
    assert_eq!(
        parse("command = 00\n"),
        Err(TraceError {
            line: 1,
            fault: Fault::UnknownDirection
        })
    );
}

#[test]
fn a_line_without_bytes_is_refused() {
    assert_eq!(
        parse("command >\n"),
        Err(TraceError {
            line: 1,
            fault: Fault::MissingBytes
        })
    );
}

#[test]
fn an_odd_number_of_hex_digits_is_refused() {
    assert_eq!(
        parse("command > abc\n"),
        Err(TraceError {
            line: 1,
            fault: Fault::OddLength
        })
    );
}

#[test]
fn a_character_outside_hex_digits_is_refused() {
    for text in ["command > zz\n", "command > +1\n", "command > 00 11\n"] {
        assert_eq!(
            parse(text),
            Err(TraceError {
                line: 1,
                fault: Fault::NotHex
            }),
            "{text}"
        );
    }
}

#[test]
fn an_empty_line_is_refused() {
    assert_eq!(
        parse("command > 00\n\ncontrol < 01\n"),
        Err(TraceError {
            line: 2,
            fault: Fault::UnknownChannel
        })
    );
}
