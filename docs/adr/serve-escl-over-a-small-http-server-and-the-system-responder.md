---
date: 2026-11-16
status: accepted
---

# Serve eSCL from a small blocking server and the system responder

## Context

The bridge must answer eSCL requests from Image Capture and sane-airscan and advertise `_uscan._tcp`.
The project code avoids async runtimes and dynamic dispatch.
sane-airscan sends `Connection: close` on every request, and AirSane closes every connection for Image Capture.

## Decision

Serve HTTP/1.1 from `std::net::TcpListener` with the `httparse` parser.
Handle one request for each connection, and send `Content-Length` and `Connection: close` with every response.
Leave the advertisement to the system responder: a static Avahi service file on Linux and `dns-sd -R` on the development Mac.

## Alternatives

`tiny_http` offers keep-alive, but it has had no release since 2022 and reads a malformed `Content-Length` as an empty body.
The `mdns-sd` crate embeds a second responder that shares the multicast port 5353 with Avahi or mDNSResponder.

## Consequences

The bridge has no keep-alive and no TLS.
Deployment installs the Avahi service file.

## Confirmation

On 2026-11-16 sane-airscan scanned through a direct URL, and Image Capture scanned through Bonjour.
On 2026-11-18 Avahi resolved port 8090 from the generated service file.
Reconsider when a client needs keep-alive or HTTPS.
