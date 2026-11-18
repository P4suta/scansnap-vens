---
date: 2026-11-16
status: accepted
---

# Reserve the scanner only during an eSCL job

## Context

The scanner holds one reserve at a time and refuses others with result `-4`.
The holder alone receives Scan button events, and ScanSnap Home may hold the reserve while it runs.

## Decision

A job request reads the paper sensors without a reserve and refuses an empty chute with status 409.
The bridge then reserves the scanner, answers with the job location, and releases the scanner when the stack ends.
A refused reserve gets status 503 at once.
Status requests report bridge state.

## Alternatives

Holding the reserve continuously would deliver button events, but it would take the scanner from ScanSnap Home.

## Consequences

Button capture needs a separate reservation policy.

## Confirmation

On 2026-11-16 an empty chute got status 409 before any reserve, and sane-airscan and Image Capture each scanned through the bridge.
On 2026-11-18 the bridge returned status 503 while ScanSnap Home on Windows held the reservation.
After ScanSnap Home exited, the bridge reserved the scanner and returned both sides of the same sheet.
Reconsider when a release request can take the reserve from ScanSnap Home.
