---
date: 2026-11-16
status: proposed
---

# Reserve the scanner only during an eSCL job

## Context

The scanner accepts one reserve at a time and refuses others with result -4.
It sends Scan button events only to the host that holds the reserve.
ScanSnap Home appears to hold the reserve while it runs, and the user keeps it on a Windows PC.

## Decision

The bridge reserves the scanner when a client posts a scan job and releases it after the job.
Capability and status requests use the cached search reply and never reserve.
A refused reserve answers the job request with status 503 after a short retry.
Holding the reserve between jobs and watching the Scan button stay out of the default.

## Alternatives

Holding the reserve continuously would deliver button events, but it would take the scanner from ScanSnap Home and can't prevent the automatic power-off.

## Consequences

An eSCL scan fails while ScanSnap Home holds the reserve.
A later button service needs its own opt-in decision.

## Confirmation

A bridge scan succeeds while ScanSnap Home holds no reserve and reports a busy scanner while it does.
Reconsider when a release request can take the reserve from ScanSnap Home.
