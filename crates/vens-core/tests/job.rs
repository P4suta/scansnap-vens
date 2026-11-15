//! The command flow follows sections 8.1, 8.3, 8.4, 8.5, and 8.7 of the ix1300-wifi protocol document.

use vens_core::{
    command::ScsiStatus,
    job::{JobError, Pause, Step, Summary, start},
    scan::Side,
    sense::Condition,
};

const GOOD: ScsiStatus = ScsiStatus::Good;
const CHECK: ScsiStatus = ScsiStatus::CheckCondition;
const END_OF_MEDIUM: u8 = 0x40;
const NO_DATA_YET: [u8; 3] = [3, 0x80, 0x13];
const NO_PAPER: [u8; 3] = [3, 0x80, 0x03];
const JAM: [u8; 3] = [3, 0x80, 0x01];
const END_OF_SIDE: [u8; 3] = [END_OF_MEDIUM, 0, 0];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Op(u8),
    Read { window: u8, page: u8, sequence: u8 },
    PixelSize { window: u8, page: u8 },
    Pause(Pause),
    Store { side: Side, bytes: Vec<u8> },
    Finish { side: Side, height: u16 },
    Done(Summary),
    Failed(JobError),
    ScriptEnded,
}

fn label(cdb: &[u8]) -> Seen {
    match cdb {
        [0x28, _, 0x80, _, _, window, _, _, _, _, page, _] => Seen::PixelSize {
            window: *window,
            page: *page,
        },
        [0x28, _, _, _, _, window, _, _, _, _, page, sequence] => Seen::Read {
            window: *window,
            page: *page,
            sequence: *sequence,
        },
        [opcode, ..] => Seen::Op(*opcode),
        [] => Seen::ScriptEnded,
    }
}

fn status(chute: u8, feed: u8) -> Vec<u8> {
    let mut reply = vec![0_u8; 0x30];
    reply
        .iter_mut()
        .skip(2)
        .zip([feed, chute])
        .for_each(|(slot, byte)| *slot = byte);
    reply
}

fn sense([flags_and_key, code, qualifier]: [u8; 3]) -> Vec<u8> {
    let mut data = vec![0_u8; 18];
    for (offset, value) in [(2, flags_and_key), (12, code), (13, qualifier)] {
        if let Some(slot) = data.get_mut(offset) {
            *slot = value;
        }
    }
    data
}

fn pixel_size(height: u32) -> Vec<u8> {
    let mut data = vec![0_u8; 32];
    data.iter_mut()
        .skip(12)
        .zip(height.to_be_bytes())
        .for_each(|(slot, byte)| *slot = byte);
    data
}

fn drive(replies: &[(ScsiStatus, Vec<u8>)]) -> Vec<Seen> {
    let mut seen = Vec::new();
    let mut replies = replies.iter();
    let mut step = start();
    loop {
        step = match step {
            Step::Execute(awaiting) => {
                seen.push(label(awaiting.command().cdb().bytes()));
                let Some((status, data)) = replies.next() else {
                    seen.push(Seen::ScriptEnded);
                    return seen;
                };
                awaiting.replied(*status, data)
            }
            Step::Pause(paused) => {
                seen.push(Seen::Pause(paused.pause()));
                paused.resumed()
            }
            Step::Store(storing) => {
                seen.push(Seen::Store {
                    side: storing.side(),
                    bytes: storing.data().to_vec(),
                });
                storing.stored()
            }
            Step::Finish(finishing) => {
                seen.push(Seen::Finish {
                    side: finishing.side(),
                    height: finishing.height(),
                });
                finishing.finished()
            }
            Step::Done(summary) => {
                seen.push(Seen::Done(summary));
                return seen;
            }
            Step::Failed(error) => {
                seen.push(Seen::Failed(error));
                return seen;
            }
        };
    }
}

fn setup() -> Vec<(ScsiStatus, Vec<u8>)> {
    [
        (GOOD, Vec::new()),
        (GOOD, Vec::new()),
        (GOOD, Vec::new()),
        (GOOD, status(0x00, 0x00)),
        (GOOD, [0_u8; 8].to_vec()),
    ]
    .to_vec()
}

fn setup_seen() -> Vec<Seen> {
    [
        Seen::Op(0xD8),
        Seen::Op(0xE9),
        Seen::Op(0xD4),
        Seen::Op(0xC2),
        Seen::Op(0xD5),
    ]
    .to_vec()
}

fn with(
    mut base: Vec<(ScsiStatus, Vec<u8>)>,
    more: &[(ScsiStatus, Vec<u8>)],
) -> Vec<(ScsiStatus, Vec<u8>)> {
    base.extend_from_slice(more);
    base
}

fn after_setup(more: &[Seen]) -> Vec<Seen> {
    let mut seen = setup_seen();
    seen.extend_from_slice(more);
    seen
}

#[test]
fn one_duplex_sheet_yields_two_corrected_sides_and_ends_the_job() {
    let replies = with(
        setup(),
        &[
            (GOOD, Vec::new()),
            (CHECK, Vec::new()),
            (GOOD, sense(NO_DATA_YET)),
            (CHECK, [0, 0, 0xFF, 0xD8, 0xAA].to_vec()),
            (GOOD, sense(END_OF_SIDE)),
            (GOOD, pixel_size(9856)),
            (GOOD, [1, 0, 0xFF, 0xD8].to_vec()),
            (CHECK, [1, 1, 0xBB].to_vec()),
            (GOOD, sense(END_OF_SIDE)),
            (GOOD, pixel_size(9000)),
            (CHECK, Vec::new()),
            (GOOD, sense(NO_PAPER)),
            (GOOD, Vec::new()),
        ],
    );
    let front = Seen::Read {
        window: 0x00,
        page: 0,
        sequence: 0,
    };
    assert_eq!(
        drive(&replies),
        after_setup(&[
            Seen::Op(0xE0),
            front.clone(),
            Seen::Op(0x03),
            Seen::Pause(Pause::NoDataYet),
            front,
            Seen::Store {
                side: Side::Front,
                bytes: [0xFF, 0xD8, 0xAA].to_vec(),
            },
            Seen::Op(0x03),
            Seen::PixelSize {
                window: 0x00,
                page: 0,
            },
            Seen::Finish {
                side: Side::Front,
                height: 9856,
            },
            Seen::Read {
                window: 0x80,
                page: 1,
                sequence: 0,
            },
            Seen::Store {
                side: Side::Back,
                bytes: [0xFF, 0xD8].to_vec(),
            },
            Seen::Read {
                window: 0x80,
                page: 1,
                sequence: 1,
            },
            Seen::Store {
                side: Side::Back,
                bytes: [0xBB].to_vec(),
            },
            Seen::Op(0x03),
            Seen::PixelSize {
                window: 0x80,
                page: 1,
            },
            Seen::Finish {
                side: Side::Back,
                height: 9000,
            },
            Seen::Op(0xE0),
            Seen::Op(0x03),
            Seen::Op(0xD6),
            Seen::Done(Summary { sheets: 1 }),
        ])
    );
}

#[test]
fn an_empty_chute_fails_before_the_job_starts() {
    let replies = [
        (GOOD, Vec::new()),
        (GOOD, Vec::new()),
        (GOOD, Vec::new()),
        (GOOD, status(0x80, 0x00)),
    ];
    assert_eq!(
        drive(&replies),
        [
            Seen::Op(0xD8),
            Seen::Op(0xE9),
            Seen::Op(0xD4),
            Seen::Op(0xC2),
            Seen::Failed(JobError::NoPaper),
        ]
    );
}

#[test]
fn a_sheet_in_the_front_slot_is_refused_until_that_path_is_supported() {
    let replies = [
        (GOOD, Vec::new()),
        (GOOD, Vec::new()),
        (GOOD, Vec::new()),
        (GOOD, status(0x81, 0x80)),
    ];
    assert_eq!(
        drive(&replies).last(),
        Some(&Seen::Failed(JobError::UnsupportedPaperPath))
    );
}

#[test]
fn a_busy_scanner_gets_the_same_command_again_after_a_pause() {
    let replies = [(ScsiStatus::Busy, Vec::new()), (GOOD, Vec::new())];
    assert_eq!(
        drive(&replies),
        [
            Seen::Op(0xD8),
            Seen::Pause(Pause::Busy),
            Seen::Op(0xD8),
            Seen::Op(0xE9),
            Seen::ScriptEnded,
        ]
    );
}

#[test]
fn no_paper_on_the_first_read_of_a_sheet_ends_the_job_normally() {
    let replies = with(
        setup(),
        &[
            (GOOD, Vec::new()),
            (CHECK, Vec::new()),
            (GOOD, sense(NO_PAPER)),
            (GOOD, Vec::new()),
        ],
    );
    assert_eq!(
        drive(&replies),
        after_setup(&[
            Seen::Op(0xE0),
            Seen::Read {
                window: 0x00,
                page: 0,
                sequence: 0,
            },
            Seen::Op(0x03),
            Seen::Op(0xD6),
            Seen::Done(Summary { sheets: 0 }),
        ])
    );
}

#[test]
fn a_jam_ends_the_job_and_reports_the_condition() {
    let replies = with(
        setup(),
        &[
            (GOOD, Vec::new()),
            (CHECK, Vec::new()),
            (GOOD, sense(JAM)),
            (GOOD, Vec::new()),
        ],
    );
    assert_eq!(
        drive(&replies).get(8..),
        Some(
            [
                Seen::Op(0xD6),
                Seen::Failed(JobError::Condition(Condition::PaperJam)),
            ]
            .as_slice()
        )
    );
}

#[test]
fn a_check_condition_during_setup_reports_its_sense_without_ending_a_job() {
    let replies = [(CHECK, Vec::new()), (GOOD, sense(JAM))];
    assert_eq!(
        drive(&replies),
        [
            Seen::Op(0xD8),
            Seen::Op(0x03),
            Seen::Failed(JobError::Condition(Condition::PaperJam)),
        ]
    );
}

#[test]
fn an_unrecognized_status_fails_with_its_code() {
    let replies = [(ScsiStatus::Unrecognized(4), Vec::new())];
    assert_eq!(
        drive(&replies),
        [
            Seen::Op(0xD8),
            Seen::Failed(JobError::Refused { status: 4 }),
        ]
    );
}

#[test]
fn a_height_beyond_the_frame_header_field_fails_the_job() {
    let replies = with(
        setup(),
        &[
            (GOOD, Vec::new()),
            (CHECK, [0, 0, 0xFF].to_vec()),
            (GOOD, sense(END_OF_SIDE)),
            (GOOD, pixel_size(65536)),
            (GOOD, Vec::new()),
        ],
    );
    assert_eq!(
        drive(&replies).get(10..),
        Some(
            [
                Seen::Op(0xD6),
                Seen::Failed(JobError::HeightOutOfRange { lines: 65536 }),
            ]
            .as_slice()
        )
    );
}

#[test]
fn a_scanner_that_never_has_data_exhausts_the_retries() {
    let mut replies = with(setup(), &[(GOOD, Vec::new())]);
    for _ in 0..200 {
        replies.push((CHECK, Vec::new()));
        replies.push((GOOD, sense(NO_DATA_YET)));
    }
    replies.push((GOOD, Vec::new()));
    let seen = drive(&replies);
    assert_eq!(
        seen.last(),
        Some(&Seen::Failed(JobError::RetriesExhausted(Pause::NoDataYet)))
    );
    assert_eq!(
        seen.iter()
            .filter(|entry| **entry == Seen::Pause(Pause::NoDataYet))
            .count(),
        100
    );
}

#[test]
fn a_second_sheet_uses_the_next_page_pair() {
    let side = |page: u8| {
        [
            (CHECK, [page, 0, 0xFF].to_vec()),
            (GOOD, sense(END_OF_SIDE)),
            (GOOD, pixel_size(100)),
        ]
    };
    let mut replies = with(setup(), &[(GOOD, Vec::new())]);
    replies.extend(side(0));
    replies.extend(side(1));
    replies.push((GOOD, Vec::new()));
    replies.extend(side(2));
    replies.extend(side(3));
    replies.extend([
        (CHECK, Vec::new()),
        (GOOD, sense(NO_PAPER)),
        (GOOD, Vec::new()),
    ]);
    let seen = drive(&replies);
    let pages: Vec<u8> = seen
        .iter()
        .filter_map(|entry| match entry {
            Seen::Read { page, .. } => Some(*page),
            Seen::Op(_)
            | Seen::PixelSize { .. }
            | Seen::Pause(_)
            | Seen::Store { .. }
            | Seen::Finish { .. }
            | Seen::Done(_)
            | Seen::Failed(_)
            | Seen::ScriptEnded => None,
        })
        .collect();
    assert_eq!(pages, [0, 1, 2, 3]);
    assert_eq!(seen.last(), Some(&Seen::Done(Summary { sheets: 2 })));
}
