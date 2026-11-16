---
date: 2026-11-16
status: proposed
---

# Serve eSCL over a small HTTP server and the system responder

## Context

The bridge must answer eSCL requests from Image Capture and sane-airscan and advertise `_uscan._tcp`.
The project code avoids async runtimes and dynamic dispatch.
sane-airscan sends `Connection: close` on every request, and AirSane closes every connection for Image Capture.

## Decision

Serve HTTP/1.1 from `std::net::TcpListener` with the `httparse` parser.
Handle one request for each connection, and send `Content-Length` and `Connection: close` with every response.
Leave the advertisement to the system responder: a static Avahi service file on Linux and `dns-sd -R` on the development Mac.

## Alternatives

`tiny_http` offers keep-alive, but it has had no release since 2022 and accepts a malformed `Content-Length` silently.
The `mdns-sd` crate embeds a second responder that shares UDP port 5353 with Avahi or mDNSResponder.

## Consequences

The bridge has no keep-alive and no TLS.
Deployment installs the Avahi service file.

## Confirmation

sane-airscan scans through a direct URL from a Linux machine.
Image Capture finds the bridge through Bonjour and scans.
Reconsider when a client needs keep-alive or HTTPS.
