//! The keepalive rounds follow sections 5.1 and 5.3 of the ix1300-wifi protocol document.
//! A round starts every 15 s, and the scanner's power save calls for up to 3 packets in each round.

use vens_core::keepalive::{Due, Elapsed, Schedule};

fn decisions(mut schedule: Schedule, events: &[(u64, bool)]) -> Vec<(u64, Due)> {
    let mut seen = Vec::new();
    for (millis, answered) in events {
        if *answered {
            schedule = schedule.answered();
            continue;
        }
        let (next, due) = schedule.poll(Elapsed::from_millis(*millis));
        schedule = next;
        seen.push((*millis, due));
    }
    seen
}

fn polls(millis: &[u64]) -> Vec<(u64, bool)> {
    millis.iter().map(|millis| (*millis, false)).collect()
}

#[test]
fn the_first_round_sends_at_once() {
    let schedule = Schedule::starting(Elapsed::from_millis(0));
    assert_eq!(decisions(schedule, &polls(&[0])), [(0, Due::Send)]);
}

#[test]
fn an_unanswered_round_sends_three_packets_half_a_second_apart() {
    let schedule = Schedule::starting(Elapsed::from_millis(0));
    assert_eq!(
        decisions(schedule, &polls(&[0, 100, 500, 999, 1000, 1500, 14_999])),
        [
            (0, Due::Send),
            (100, Due::Wait),
            (500, Due::Send),
            (999, Due::Wait),
            (1000, Due::Send),
            (1500, Due::Wait),
            (14_999, Due::Wait),
        ]
    );
}

#[test]
fn an_answer_ends_the_round() {
    let schedule = Schedule::starting(Elapsed::from_millis(0));
    assert_eq!(
        decisions(
            schedule,
            &[(0, false), (0, true), (500, false), (1000, false)]
        ),
        [(0, Due::Send), (500, Due::Wait), (1000, Due::Wait)]
    );
}

#[test]
fn the_next_round_starts_fifteen_seconds_after_the_last_one() {
    let schedule = Schedule::starting(Elapsed::from_millis(0));
    assert_eq!(
        decisions(
            schedule,
            &[
                (0, false),
                (0, true),
                (14_999, false),
                (15_000, false),
                (15_100, false),
                (15_500, false),
            ]
        ),
        [
            (0, Due::Send),
            (14_999, Due::Wait),
            (15_000, Due::Send),
            (15_100, Due::Wait),
            (15_500, Due::Send),
        ]
    );
}

#[test]
fn a_late_poll_joins_the_current_round_without_a_burst() {
    let schedule = Schedule::starting(Elapsed::from_millis(0));
    assert_eq!(
        decisions(
            schedule,
            &[
                (0, false),
                (0, true),
                (47_000, false),
                (47_100, false),
                (47_500, false),
                (60_000, false),
            ]
        ),
        [
            (0, Due::Send),
            (47_000, Due::Send),
            (47_100, Due::Wait),
            (47_500, Due::Send),
            (60_000, Due::Send),
        ]
    );
}

#[test]
fn a_schedule_that_starts_later_counts_its_rounds_from_its_start() {
    let schedule = Schedule::starting(Elapsed::from_millis(2_000));
    assert_eq!(
        decisions(
            schedule,
            &[
                (2_000, false),
                (2_000, true),
                (16_999, false),
                (17_000, false)
            ]
        ),
        [(2_000, Due::Send), (16_999, Due::Wait), (17_000, Due::Send)]
    );
}
