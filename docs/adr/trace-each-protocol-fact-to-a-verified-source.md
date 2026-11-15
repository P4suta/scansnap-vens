---
date: 2026-11-16
status: proposed
---

# Trace each protocol fact to a verified source

## Context

No official specification exists.
The [protocol document by zer0pwned](https://github.com/zer0pwned/ix1300-wifi/blob/main/PROTOCOL.md) carries a permissive license and marks the sections verified on hardware.
The [AirScap protocol document](https://github.com/mzyy94/AirScap/blob/master/protocol.en.md) carries a permissive license and describes an older model.
Other public sources have no license or a copyleft license.

## Decision

Use only facts that trace to a hardware-verified section of those documents, a published standard, or this project's own capture.
Copy no code and no prose from any source.
Keep the unlicensed and copyleft implementations out of the implementation work.
Name the source section in the test that fixes each frame layout.

## Alternatives

Porting an existing implementation would import license terms and assumptions that nobody tested on this model.

## Consequences

Where the documents disagree, the hardware decides, and the test records the observed bytes.
Unverified sections stay unimplemented until a capture confirms them.
Reconsider this rule when the vendor publishes a specification.

## Confirmation

Each golden frame test cites its source section.
The record stays proposed until the first hardware scan confirms the cited layouts.
