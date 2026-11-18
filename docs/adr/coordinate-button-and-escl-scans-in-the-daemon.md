---
date: 2026-11-18
status: proposed
---

# Coordinate button and eSCL scans in the daemon

## Context

The scanner sends button notifications to the host that holds its reservation.
The eSCL bridge acquires a reservation for each requested scan by default.
The iX1300 protocol describes how the reservation directs notifications.
[iX1300 protocol, sections 5.3 and 6](https://github.com/zer0pwned/ix1300-wifi/blob/main/PROTOCOL.md#6-trigger-packets-button-and-power-events-hw)

## Decision

An explicit consumption folder enables a scanner worker in the daemon.
The worker keeps the reservation alive while it waits for button notifications or eSCL scan requests.
It runs one scan at a time and keeps the recent button sequence numbers across scan jobs.

## Alternatives

Separate daemons would race for a reservation.
Polling the paper sensors without a reservation misses the network button events.

## Consequences

Button capture takes exclusive use of the scanner while it waits.
After a job the worker releases and acquires the reservation again.
Reconsider if the scanner can deliver button events without a reservation.

## Confirmation

On 2026-11-18 one Scan button press saved one duplex document.
With capture enabled, the daemon answered eSCL status requests and refused a scan when the chute was empty.
On 2026-11-18 the same worker served both sides of a sheet over eSCL and resumed waiting for the button.
