---
date: 2026-11-16
status: accepted
---

# Verify with an emulator and explicit hardware runs

## Context

One physical scanner exists, and hosted runners have none.
The protocol documents rest on a single device.
A check that needs paper in a feeder can't gate every commit.

## Decision

Build an in-process emulator that replays redacted hardware traces.
Run every gate test on the emulator with a virtual clock.
Run hardware checks only through an explicit `vens` command, such as `just vens scan`.
Hooks and hosted workflows never run them.
Redact host identifiers, addresses, serial numbers, and image payloads before a trace enters the repository.

## Alternatives

Mocking only at the port traits is cheaper, but it can't expose a framing error.
A self-hosted runner with the scanner attached would test every change, but it needs paper and an attended device.

## Consequences

The emulator can repeat a mistake of the documents until a trace corrects it.
Each milestone states which behavior has hardware evidence.
Reconsider this split when a second scanner model joins.

## Confirmation

`mise x -- just check` passes on a machine without the scanner.
On 2026-11-16 the emulator replayed a redacted duplex hardware trace and produced both images at the recorded height.
