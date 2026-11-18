//! The bridge decides every eSCL response from its job state and the events of the job thread.
//! Clients learn the end of a job from the final `NextDocument` 404 and the status that precedes it, so the status switches before that response.

use std::fmt::Debug;

use vens_escl::{
    bridge::{
        Bridge, Conflict, Effect, Ending, Fault, HINT_MILLIS, Input, Moment, RETENTION_MILLIS,
        Refusal, Request, Response, Side, Unavailable,
    },
    document::{AdfState, Capabilities, JobInfo, JobState, ScannerState, ScannerStatus},
    route::{Method, Unrouted},
    settings::{JobSettings, Requested, Unsupported, accept},
    uuid::Uuid,
};

type Bridge4 = Bridge<&'static str, &'static str, 4>;
type Effects = Vec<Effect<&'static str, &'static str>>;
type Log = Vec<Effects>;

const UUID: Uuid = Uuid::from_bytes([0x11; 16]);
const SEED: [u8; 16] = [0x22; 16];
const OTHER: Uuid = Uuid::from_bytes([0x33; 16]);

fn duplex() -> Result<JobSettings, Unsupported> {
    accept(&Requested {
        duplex: Some("true"),
        ..Requested::default()
    })
}

fn simplex() -> Result<JobSettings, Unsupported> {
    accept(&Requested::default())
}

fn request(request: Request, responder: &'static str) -> Input<&'static str, &'static str> {
    Input::Request { request, responder }
}

fn page(side: Side, page: &'static str) -> Input<&'static str, &'static str> {
    Input::Page { side, page }
}

/// Feeds the inputs at their moments and returns the bridge with the effects of each input.
fn feed<const N: usize>(
    bridge: Bridge<&'static str, &'static str, N>,
    inputs: impl IntoIterator<Item = (u64, Input<&'static str, &'static str>)>,
) -> (Bridge<&'static str, &'static str, N>, Log) {
    inputs.into_iter().fold(
        (bridge, Log::new()),
        |(bridge, mut log), (millis, input)| {
            let (bridge, effects) = bridge.handle(input, Moment::from_millis(millis));
            let (bridge, replies) =
                effects
                    .into_iter()
                    .fold(
                        (bridge, Vec::new()),
                        |(bridge, mut replies), effect| match effect {
                            Effect::Transfer {
                                responder,
                                id,
                                page,
                            } => {
                                let (bridge, _) = bridge.handle(
                                    Input::PageWritten { id, returned: None },
                                    Moment::from_millis(millis),
                                );
                                replies.push(Effect::Reply {
                                    responder,
                                    response: Response::Page(page),
                                });
                                (bridge, replies)
                            }
                            Effect::Fault {
                                responder,
                                id,
                                fault,
                            } => {
                                let (bridge, _) = bridge
                                    .handle(Input::FaultWritten(id), Moment::from_millis(millis));
                                replies.push(Effect::Reply {
                                    responder,
                                    response: Response::Conflict(Conflict::Failed(fault)),
                                });
                                (bridge, replies)
                            }
                            effect @ (Effect::Reply { .. } | Effect::Start(_)) => {
                                replies.push(effect);
                                (bridge, replies)
                            }
                        },
                    );
            log.push(replies);
            (bridge, log)
        },
    )
}

fn reply(responder: &'static str, response: Response<&'static str>) -> Effects {
    [Effect::Reply {
        responder,
        response,
    }]
    .to_vec()
}

fn start() -> Effects {
    duplex().map(Effect::Start).into_iter().collect()
}

fn status_of<const N: usize>(
    bridge: &Bridge<&'static str, &'static str, N>,
    millis: u64,
) -> Option<ScannerStatus> {
    let (_, effects) = bridge.clone().handle(
        request(Request::Status, "status"),
        Moment::from_millis(millis),
    );
    effects.into_iter().find_map(|effect| match effect {
        Effect::Reply {
            response: Response::Status(status),
            ..
        } => Some(status),
        Effect::Reply { .. }
        | Effect::Start(_)
        | Effect::Transfer { .. }
        | Effect::Fault { .. } => None,
    })
}

fn created(effects: Option<&Effects>) -> Option<Uuid> {
    effects?.iter().find_map(|effect| match effect {
        Effect::Reply {
            response: Response::Created(path),
            ..
        } => Some(path.job()),
        Effect::Reply { .. }
        | Effect::Start(_)
        | Effect::Transfer { .. }
        | Effect::Fault { .. } => None,
    })
}

/// Starts a job and returns the bridge with the job's identifier from its location.
fn started<const N: usize>(
    settings: Result<JobSettings, Unsupported>,
) -> Option<(Bridge<&'static str, &'static str, N>, Uuid)> {
    let (bridge, log) = feed(
        Bridge::new(UUID, SEED),
        [
            (0, request(Request::CreateJob(settings), "post")),
            (1, Input::Reserved),
        ],
    );
    created(log.last()).map(|id| (bridge, id))
}

/// Compares what a started job produced with what it should produce, and fails when the job never started.
fn check<T: PartialEq + Debug>(checks: Option<(T, T)>) {
    let (actual, expected) = checks.unzip();
    assert!(actual.is_some(), "the job never started");
    assert_eq!(actual, expected);
}

const fn info(id: Uuid, completed: u32, to_transfer: u32, state: JobState) -> JobInfo {
    JobInfo {
        id,
        images_completed: completed,
        images_to_transfer: to_transfer,
        state,
    }
}

const IDLE: Option<ScannerStatus> = Some(ScannerStatus {
    state: ScannerState::Idle,
    adf: AdfState::Loaded,
    job: None,
});

#[test]
fn the_capabilities_carry_the_bridge_identifier() {
    let (_, log) = feed(
        Bridge4::new(UUID, SEED),
        [(0, request(Request::Capabilities, "caps"))],
    );
    assert_eq!(
        log,
        [reply(
            "caps",
            Response::Capabilities(Capabilities::new(UUID))
        )]
    );
}

#[test]
fn an_idle_bridge_reports_an_idle_scanner_with_a_loaded_feeder() {
    assert_eq!(status_of(&Bridge4::new(UUID, SEED), 0), IDLE);
}

#[test]
fn unrouted_and_malformed_requests_get_their_refusals() {
    let (_, log) = feed(
        Bridge4::new(UUID, SEED),
        [
            (0, request(Request::Unrouted(Unrouted::NotFound), "a")),
            (
                0,
                request(
                    Request::Unrouted(Unrouted::MethodNotAllowed(Method::Post)),
                    "b",
                ),
            ),
            (0, request(Request::Malformed, "c")),
        ],
    );
    assert_eq!(
        log,
        [
            reply("a", Response::NotFound),
            reply("b", Response::MethodNotAllowed(Method::Post)),
            reply("c", Response::BadRequest),
        ]
    );
}

#[test]
fn a_job_request_starts_the_job_and_waits_for_the_reserve() {
    let (_, log) = feed(
        Bridge4::new(UUID, SEED),
        [(0, request(Request::CreateJob(duplex()), "post"))],
    );
    assert_eq!(log, [start()]);
}

#[test]
fn the_reserve_answers_the_job_request_and_the_status_shows_the_job() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        (
            status_of(&bridge, 2),
            Some(ScannerStatus {
                state: ScannerState::Processing,
                adf: AdfState::Loaded,
                job: Some(info(id, 0, 0, JobState::Processing)),
            }),
        )
    }));
}

#[test]
fn each_job_gets_its_own_identifier() {
    let next = started::<4>(duplex()).and_then(|(bridge, first)| {
        let (_, log) = feed(
            bridge,
            [
                (2, Input::Finished(Ending::Completed)),
                (3, request(Request::CreateJob(duplex()), "post")),
                (4, Input::Reserved),
            ],
        );
        created(log.last()).map(|second| second != first)
    });
    assert_eq!(next, Some(true));
}

#[test]
fn an_unsupported_setting_is_refused_without_a_job() {
    let (bridge, log) = feed(
        Bridge4::new(UUID, SEED),
        [(
            0,
            request(Request::CreateJob(Err(Unsupported::ColorMode)), "post"),
        )],
    );
    assert_eq!(
        (log, status_of(&bridge, 1)),
        (
            [reply(
                "post",
                Response::Conflict(Conflict::Unsupported(Unsupported::ColorMode))
            )]
            .to_vec(),
            IDLE
        )
    );
}

#[test]
fn each_refusal_answers_the_job_request_and_frees_the_bridge() {
    let answers = [
        Refusal::NoPaper,
        Refusal::Unregistered,
        Refusal::Busy,
        Refusal::Unreachable,
    ]
    .map(|refusal| {
        let (_, log) = feed(
            Bridge4::new(UUID, SEED),
            [
                (0, request(Request::CreateJob(duplex()), "post")),
                (1, Input::Refused(refusal)),
                (2, request(Request::CreateJob(duplex()), "again")),
            ],
        );
        log.split_at_checked(1).map(|(_, rest)| rest.to_vec())
    });
    assert_eq!(
        answers,
        [
            Response::Conflict(Conflict::NoPaper),
            Response::Conflict(Conflict::Unregistered),
            Response::Unavailable(Unavailable::ScannerBusy),
            Response::Unavailable(Unavailable::ScannerUnreachable),
        ]
        .map(|response| Some([reply("post", response), start()].to_vec()))
    );
}

#[test]
fn a_refusal_hint_never_returns_after_a_later_job() {
    let (bridge, log) = feed(
        Bridge4::new(UUID, SEED),
        [
            (0, request(Request::CreateJob(duplex()), "post")),
            (1, Input::Refused(Refusal::NoPaper)),
            (1000, request(Request::CreateJob(duplex()), "again")),
            (1001, Input::Reserved),
        ],
    );
    let status = created(log.last()).map(|id| {
        let (bridge, _) = feed(
            bridge,
            [
                (1002, Input::Finished(Ending::Completed)),
                (1003, request(Request::DeleteJob(id), "delete")),
            ],
        );
        status_of(&bridge, 1004)
    });
    assert_eq!(status, Some(IDLE));
}

#[test]
fn a_drained_job_that_nobody_deletes_shows_its_end_for_the_hint_time() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, _) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, Input::Finished(Ending::Completed)),
                (4, request(Request::NextDocument(id), "page")),
                (5, request(Request::NextDocument(id), "end")),
            ],
        );
        (
            (
                status_of(&bridge, 5 + HINT_MILLIS - 1),
                status_of(&bridge, 5 + HINT_MILLIS),
            ),
            (
                Some(ScannerStatus {
                    state: ScannerState::Idle,
                    adf: AdfState::Empty,
                    job: Some(info(id, 1, 0, JobState::Completed)),
                }),
                IDLE,
            ),
        )
    }));
}

#[test]
fn a_page_request_on_a_finished_job_restarts_its_retention() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, _) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, page(Side::Back, "back")),
                (4, Input::Finished(Ending::Completed)),
                (
                    4 + RETENTION_MILLIS - 1,
                    request(Request::NextDocument(id), "late"),
                ),
            ],
        );
        (
            status_of(&bridge, 4 + RETENTION_MILLIS + 5),
            Some(ScannerStatus {
                state: ScannerState::Processing,
                adf: AdfState::Loaded,
                job: Some(info(id, 1, 1, JobState::Processing)),
            }),
        )
    }));
}

#[test]
fn requests_for_another_job_leave_the_running_job_in_place() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, _) = feed(
            bridge,
            [
                (2, request(Request::NextDocument(OTHER), "next")),
                (3, request(Request::DeleteJob(OTHER), "delete")),
                (4, page(Side::Front, "front")),
            ],
        );
        (
            status_of(&bridge, 5),
            Some(ScannerStatus {
                state: ScannerState::Processing,
                adf: AdfState::Loaded,
                job: Some(info(id, 0, 1, JobState::Processing)),
            }),
        )
    }));
}

#[test]
fn an_empty_chute_shows_in_the_status_until_its_hint_expires() {
    let (bridge, _) = feed(
        Bridge4::new(UUID, SEED),
        [
            (0, request(Request::CreateJob(duplex()), "post")),
            (1000, Input::Refused(Refusal::NoPaper)),
        ],
    );
    assert_eq!(
        (
            status_of(&bridge, 1000 + HINT_MILLIS - 1).map(|status| status.adf),
            status_of(&bridge, 1000 + HINT_MILLIS).map(|status| status.adf),
        ),
        (Some(AdfState::Empty), Some(AdfState::Loaded))
    );
}

#[test]
fn a_second_job_request_during_a_job_is_unavailable() {
    let (_, log) = feed(
        Bridge4::new(UUID, SEED),
        [
            (0, request(Request::CreateJob(duplex()), "post")),
            (1, request(Request::CreateJob(duplex()), "early")),
            (2, Input::Reserved),
            (3, request(Request::CreateJob(duplex()), "late")),
        ],
    );
    assert_eq!(
        [log.get(1), log.get(3)],
        [
            Some(&reply(
                "early",
                Response::Unavailable(Unavailable::JobInProgress)
            )),
            Some(&reply(
                "late",
                Response::Unavailable(Unavailable::JobInProgress)
            )),
        ]
    );
}

#[test]
fn pages_arrive_in_order_and_a_waiting_request_gets_the_next_page() {
    let log = started::<4>(duplex()).map(|(bridge, id)| {
        feed(
            bridge,
            [
                (2, request(Request::NextDocument(id), "first")),
                (3, page(Side::Front, "front")),
                (4, page(Side::Back, "back")),
                (5, request(Request::NextDocument(id), "second")),
            ],
        )
        .1
    });
    assert_eq!(
        log,
        Some(
            [
                Effects::new(),
                reply("first", Response::Page("front")),
                Effects::new(),
                reply("second", Response::Page("back")),
            ]
            .to_vec()
        )
    );
}

#[test]
fn a_one_sided_job_drops_the_back_sides() {
    let log = started::<4>(simplex()).map(|(bridge, id)| {
        feed(
            bridge,
            [
                (2, page(Side::Front, "front 1")),
                (3, page(Side::Back, "back 1")),
                (4, page(Side::Front, "front 2")),
                (5, page(Side::Back, "back 2")),
                (6, Input::Finished(Ending::Completed)),
                (7, request(Request::NextDocument(id), "a")),
                (8, request(Request::NextDocument(id), "b")),
                (9, request(Request::NextDocument(id), "c")),
            ],
        )
        .1
        .split_off(5)
    });
    assert_eq!(
        log,
        Some(
            [
                reply("a", Response::Page("front 1")),
                reply("b", Response::Page("front 2")),
                reply("c", Response::NotFound),
            ]
            .to_vec()
        )
    );
}

#[test]
fn the_last_page_switches_the_status_to_completed_before_the_final_not_found() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, _) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, Input::Finished(Ending::Completed)),
            ],
        );
        let transferring = status_of(&bridge, 4);
        let (bridge, delivered) = feed(bridge, [(5, request(Request::NextDocument(id), "page"))]);
        let completed = status_of(&bridge, 6);
        let (_, end) = feed(bridge, [(7, request(Request::NextDocument(id), "end"))]);
        (
            (transferring, delivered, completed, end),
            (
                Some(ScannerStatus {
                    state: ScannerState::Processing,
                    adf: AdfState::Loaded,
                    job: Some(info(id, 0, 1, JobState::Processing)),
                }),
                [reply("page", Response::Page("front"))].to_vec(),
                Some(ScannerStatus {
                    state: ScannerState::Idle,
                    adf: AdfState::Empty,
                    job: Some(info(id, 1, 0, JobState::Completed)),
                }),
                [reply("end", Response::NotFound)].to_vec(),
            ),
        )
    }));
}

#[test]
fn a_waiting_request_learns_the_end_of_the_job() {
    let log = started::<4>(duplex()).map(|(bridge, id)| {
        feed(
            bridge,
            [
                (2, request(Request::NextDocument(id), "waiting")),
                (3, Input::Finished(Ending::Completed)),
            ],
        )
        .1
    });
    assert_eq!(
        log,
        Some([Effects::new(), reply("waiting", Response::NotFound)].to_vec())
    );
}

#[test]
fn a_failed_job_delivers_its_pages_and_then_a_conflict() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, mut log) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, Input::Finished(Ending::Failed(Fault::Jam))),
                (4, request(Request::NextDocument(id), "page")),
                (5, request(Request::NextDocument(id), "failed")),
            ],
        );
        (
            (log.split_off(2), status_of(&bridge, 6)),
            (
                [
                    reply("page", Response::Page("front")),
                    reply("failed", Response::Conflict(Conflict::Failed(Fault::Jam))),
                ]
                .to_vec(),
                Some(ScannerStatus {
                    state: ScannerState::Idle,
                    adf: AdfState::Jam,
                    job: Some(info(id, 1, 0, JobState::Aborted)),
                }),
            ),
        )
    }));
}

#[test]
fn a_drained_failure_waits_for_the_clients_next_page_request() {
    let answer = started::<4>(duplex()).map(|(bridge, id)| {
        let (_, log) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, Input::Finished(Ending::Failed(Fault::Jam))),
                (4, request(Request::NextDocument(id), "page")),
                (
                    RETENTION_MILLIS.saturating_mul(2),
                    request(Request::NextDocument(id), "fault"),
                ),
            ],
        );
        log.into_iter().last()
    });
    assert_eq!(
        answer.flatten(),
        Some(reply(
            "fault",
            Response::Conflict(Conflict::Failed(Fault::Jam))
        ))
    );
}

#[test]
fn an_unwritten_fault_still_answers_a_retry_after_the_hint_interval() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (waiting, _) = bridge.handle(
            request(Request::NextDocument(id), "gone"),
            Moment::from_millis(2),
        );
        let (ended, _) = waiting.handle(
            Input::Finished(Ending::Failed(Fault::Jam)),
            Moment::from_millis(3),
        );
        let retry = HINT_MILLIS.saturating_add(4);
        let next = request(Request::NextDocument(id), "retry");
        let (ended, _) = ended.handle(next.clone(), Moment::from_millis(retry));
        let written = retry.saturating_add(HINT_MILLIS);
        let (_, replies) = feed(
            ended,
            [
                (written, next.clone()),
                (written.saturating_add(HINT_MILLIS), next),
            ],
        );
        (
            replies,
            [
                reply("retry", Response::Conflict(Conflict::Failed(Fault::Jam))),
                reply("retry", Response::NotFound),
            ]
            .to_vec(),
        )
    }));
}

#[test]
fn a_failed_write_keeps_the_same_page_at_the_front_of_the_job() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, _) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, page(Side::Back, "back")),
                (4, Input::Finished(Ending::Completed)),
            ],
        );
        let (bridge, first) = bridge.handle(
            request(Request::NextDocument(id), "first"),
            Moment::from_millis(5),
        );
        let sent = |responder| Effect::Transfer {
            responder,
            id,
            page: "front",
        };
        let before = status_of(&bridge, 6).and_then(|status| status.job);
        let (bridge, _) = bridge.handle(
            Input::PageWritten {
                id,
                returned: Some("front"),
            },
            Moment::from_millis(7),
        );
        let (bridge, retry) = bridge.handle(
            request(Request::NextDocument(id), "retry"),
            Moment::from_millis(8),
        );
        let (bridge, _) = bridge.handle(
            Input::PageWritten { id, returned: None },
            Moment::from_millis(9),
        );
        let after = status_of(&bridge, 10).and_then(|status| status.job);
        (
            (
                first.into_iter().next(),
                retry.into_iter().next(),
                before,
                after,
            ),
            (
                Some(sent("first")),
                Some(sent("retry")),
                Some(info(id, 0, 2, JobState::Processing)),
                Some(info(id, 1, 1, JobState::Processing)),
            ),
        )
    }));
}

#[test]
fn each_fault_shows_its_feeder_state_after_the_kept_pages() {
    let faults = [
        (Fault::Jam, AdfState::Jam),
        (Fault::DoubleFeed, AdfState::MultipickDetected),
        (Fault::Scanner, AdfState::Loaded),
        (Fault::Lost, AdfState::Loaded),
    ];
    for (fault, adf) in faults {
        check(started::<4>(duplex()).map(|(bridge, id)| {
            let (bridge, mut log) = feed(
                bridge,
                [
                    (2, Input::Finished(Ending::Failed(fault))),
                    (3, request(Request::NextDocument(id), "failed")),
                ],
            );
            (
                (log.split_off(1), status_of(&bridge, 4)),
                (
                    [reply("failed", Response::Conflict(Conflict::Failed(fault)))].to_vec(),
                    Some(ScannerStatus {
                        state: ScannerState::Idle,
                        adf,
                        job: Some(info(id, 0, 0, JobState::Aborted)),
                    }),
                ),
            )
        }));
    }
}

#[test]
fn deleting_a_running_job_answers_its_waiting_request_and_frees_the_bridge_after_the_scan() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, mut log) = feed(
            bridge,
            [
                (2, request(Request::NextDocument(id), "waiting")),
                (3, request(Request::DeleteJob(id), "delete")),
                (4, page(Side::Front, "front")),
                (5, request(Request::CreateJob(duplex()), "busy")),
            ],
        );
        let canceled = status_of(&bridge, 6);
        let (bridge, _) = feed(bridge, [(7, Input::Finished(Ending::Completed))]);
        let freed = status_of(&bridge, 8);
        let (_, again) = feed(
            bridge,
            [(9, request(Request::CreateJob(duplex()), "again"))],
        );
        (
            (log.split_off(1), canceled, freed, again),
            (
                [
                    [
                        Effect::Reply {
                            responder: "delete",
                            response: Response::Deleted,
                        },
                        Effect::Reply {
                            responder: "waiting",
                            response: Response::NotFound,
                        },
                    ]
                    .to_vec(),
                    Effects::new(),
                    reply("busy", Response::Unavailable(Unavailable::JobInProgress)),
                ]
                .to_vec(),
                Some(ScannerStatus {
                    state: ScannerState::Processing,
                    adf: AdfState::Loaded,
                    job: Some(info(id, 0, 0, JobState::Canceled)),
                }),
                IDLE,
                [start()].to_vec(),
            ),
        )
    }));
}

#[test]
fn deleting_a_finished_job_frees_the_bridge_at_once() {
    let checks = started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, mut log) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, Input::Finished(Ending::Completed)),
                (4, request(Request::DeleteJob(id), "delete")),
            ],
        );
        (log.split_off(2), status_of(&bridge, 5))
    });
    assert_eq!(
        checks,
        Some(([reply("delete", Response::Deleted)].to_vec(), IDLE))
    );
}

#[test]
fn requests_for_another_job_or_no_job_are_not_found() {
    let expected = [
        reply("next", Response::NotFound),
        reply("delete", Response::NotFound),
    ]
    .to_vec();
    let (_, idle) = feed(
        Bridge4::new(UUID, SEED),
        [
            (0, request(Request::NextDocument(OTHER), "next")),
            (0, request(Request::DeleteJob(OTHER), "delete")),
        ],
    );
    let running = started::<4>(duplex()).map(|(bridge, _)| {
        feed(
            bridge,
            [
                (2, request(Request::NextDocument(OTHER), "next")),
                (3, request(Request::DeleteJob(OTHER), "delete")),
            ],
        )
        .1
    });
    assert_eq!((idle, running), (expected.clone(), Some(expected)));
}

#[test]
fn a_finished_job_with_pending_pages_holds_the_scanner_until_its_retention_ends() {
    check(started::<4>(duplex()).map(|(bridge, id)| {
        let (bridge, refused) = feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, Input::Finished(Ending::Completed)),
                (4, request(Request::CreateJob(duplex()), "busy")),
            ],
        );
        let (_, expired) = feed(
            bridge.clone(),
            [(
                3 + RETENTION_MILLIS,
                request(Request::CreateJob(duplex()), "later"),
            )],
        );
        (
            (
                refused.last().cloned(),
                status_of(&bridge, 3 + RETENTION_MILLIS - 1),
                status_of(&bridge, 3 + RETENTION_MILLIS),
                expired,
            ),
            (
                Some(reply(
                    "busy",
                    Response::Unavailable(Unavailable::JobInProgress),
                )),
                Some(ScannerStatus {
                    state: ScannerState::Processing,
                    adf: AdfState::Loaded,
                    job: Some(info(id, 0, 1, JobState::Processing)),
                }),
                IDLE,
                [start()].to_vec(),
            ),
        )
    }));
}

#[test]
fn a_delivered_job_yields_the_scanner_to_the_next_job_request() {
    let log = started::<4>(duplex()).map(|(bridge, id)| {
        feed(
            bridge,
            [
                (2, page(Side::Front, "front")),
                (3, Input::Finished(Ending::Completed)),
                (4, request(Request::NextDocument(id), "page")),
                (5, request(Request::CreateJob(duplex()), "next")),
            ],
        )
        .1
        .split_off(3)
    });
    assert_eq!(log, Some([start()].to_vec()));
}

#[test]
fn a_newer_waiting_request_supersedes_an_older_one() {
    let log = started::<4>(duplex()).map(|(bridge, id)| {
        feed(
            bridge,
            [
                (2, request(Request::NextDocument(id), "older")),
                (3, request(Request::NextDocument(id), "newer")),
                (4, page(Side::Front, "front")),
            ],
        )
        .1
    });
    assert_eq!(
        log,
        Some(
            [
                Effects::new(),
                reply("older", Response::Unavailable(Unavailable::Superseded)),
                reply("newer", Response::Page("front")),
            ]
            .to_vec()
        )
    );
}

#[test]
fn the_page_queue_keeps_its_order_across_the_wrap() {
    let log = started::<2>(duplex()).map(|(bridge, id)| {
        feed(
            bridge,
            [
                (2, page(Side::Front, "1")),
                (3, page(Side::Back, "2")),
                (4, request(Request::NextDocument(id), "a")),
                (5, page(Side::Front, "3")),
                (6, request(Request::NextDocument(id), "b")),
                (7, request(Request::NextDocument(id), "c")),
            ],
        )
        .1
        .split_off(2)
    });
    assert_eq!(
        log,
        Some(
            [
                reply("a", Response::Page("1")),
                Effects::new(),
                reply("b", Response::Page("2")),
                reply("c", Response::Page("3")),
            ]
            .to_vec()
        )
    );
}

#[test]
fn a_full_queue_drops_the_page_and_fails_the_job_after_the_kept_pages() {
    let log = started::<2>(duplex()).map(|(bridge, id)| {
        feed(
            bridge,
            [
                (2, page(Side::Front, "1")),
                (3, page(Side::Back, "2")),
                (4, page(Side::Front, "3")),
                (5, Input::Finished(Ending::Completed)),
                (6, request(Request::NextDocument(id), "a")),
                (7, request(Request::NextDocument(id), "b")),
                (8, request(Request::NextDocument(id), "c")),
            ],
        )
        .1
        .split_off(4)
    });
    assert_eq!(
        log,
        Some(
            [
                reply("a", Response::Page("1")),
                reply("b", Response::Page("2")),
                reply("c", Response::Conflict(Conflict::Failed(Fault::Lost))),
            ]
            .to_vec()
        )
    );
}

#[test]
fn job_thread_events_without_a_matching_job_change_nothing() {
    let (bridge, log) = feed(
        Bridge4::new(UUID, SEED),
        [
            (0, Input::Reserved),
            (0, Input::Refused(Refusal::Busy)),
            (0, page(Side::Front, "stray")),
            (0, Input::Finished(Ending::Completed)),
        ],
    );
    assert_eq!(
        (log, status_of(&bridge, 1)),
        (
            [
                Effects::new(),
                Effects::new(),
                Effects::new(),
                Effects::new()
            ]
            .to_vec(),
            IDLE
        )
    );
}
