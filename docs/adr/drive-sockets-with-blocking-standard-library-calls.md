---
date: 2026-11-16
status: proposed
---

# Drive sockets with blocking standard library calls

## Context

The scanner interface is strict request and reply on one connection.
Only the keepalive datagram and the button listener run beside a job.
The type policy refuses shared handles, locks, and trait objects.

## Decision

Use blocking `std::net` sockets with read and write deadlines.
Express each port as a trait that callers pass through a generic parameter.
Run the keepalive and the listener on scoped threads that exchange values over channels.

## Alternatives

An asynchronous runtime offers timers and cancellation.
It also brings a scheduler, boxed tasks, and shared handles, which conflict with the type policy.
A single thread without a keepalive loses the reserve during long jobs.

## Consequences

Cancellation depends on socket deadlines, so every blocking call needs one.
The daemon serves one scanner and one job at a time.
Reconsider this choice if the bridge must serve many scanners or many clients at once.

## Confirmation

Emulator tests exercise deadlines under a virtual clock.
The record stays proposed until a hardware scan with a keepalive completes.
