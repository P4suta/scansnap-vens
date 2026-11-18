---
date: 2026-11-16
status: accepted
---

# Reuse the registered host identifier

## Context

The scanner stores one registered host identifier of eight bytes.
A reserve from any other identifier fails with the result `-6`.
The vendor software registers its own identifier whenever it connects.
The hardware status command returns the registered identifier without a reserve.

## Decision

Read the registered identifier before each reserve and reserve with that value.
Never write an identifier during a normal scan.
Offer registration only as a separate explicit command for a scanner that reports no identifier.

## Alternatives

Registering an identifier that this project owns would make the vendor software fail until it registers again.
The two programs would then overwrite each other on every switch.

## Consequences

Any host on the network that can read the identifier can reserve the scanner.
The scanner already works this way, and this project adds no secret to it.
Captured traces replace the identifier before they enter the repository.
Reconsider this choice if a firmware update stops returning the identifier without a reserve.

## Confirmation

The first hardware run used the registered identifier to scan a sheet.
On 2026-11-18 a duplex scan succeeded after ScanSnap Home on Windows released the reservation.
