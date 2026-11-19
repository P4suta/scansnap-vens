---
date: 2026-11-19
status: proposed
---

# Register eSCL with the Windows responder

## Context

The [system responder decision](serve-escl-over-a-small-http-server-and-the-system-responder.md) covers Avahi and Bonjour.
A native Windows build also needs to publish the bridge for local scan clients.

## Decision

Register the existing service type and text record with the [Windows service registration API](https://learn.microsoft.com/en-us/windows/win32/api/windns/nf-windns-dnsserviceregister).
Start registration after the HTTP socket binds and keep the callback inputs alive until completion.
The Windows process owns the registration.

## Alternatives

An embedded responder adds another listener to the multicast port.
An installed Bonjour runtime would add a dependency to the Windows archive.

## Consequences

A Windows adapter owns the native calls and exposes a safe Rust interface.
Report registration errors and keep the HTTP endpoint available by address.
Reconsider if the native responder can't publish the listening addresses.

## Confirmation

The [Windows integration test](../../crates/vens-daemon/tests/windows_discovery.rs) resolves the service and checks its identifier in `ScannerCapabilities`.
