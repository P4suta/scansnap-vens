---
date: 2026-11-16
status: proposed
---

# Keep protocol decisions in a dependency-free core

## Context

The scanner speaks a private binary protocol that no vendor document describes.
Its rules come from reverse-engineered notes, so each one needs tests that run without hardware.
The project requires static dispatch, exhaustive handling, and no shared ownership.

## Decision

Put the frame codec, command builders, sense decoding, and the scan job state machine in `vens-core`.
The crate uses `#![no_std]`, has no dependencies, and allocates nothing.
The job advances through a pure `step` function that returns the next action.
Adapters in other crates perform every effect.

## Alternatives

A single crate that mixes sockets with decisions is the smallest start.
It would let heap types and panics enter unnoticed.
An asynchronous client would add boxed futures that the type policy refuses.

## Consequences

Callers supply buffers, so image data never lives in the core.
Property test crates live in a separate conformance package.
Reconsider the allocation ban if a required frame can't fit a caller-provided buffer.

## Confirmation

`mise x -- just check` compiles the core without the standard library and runs the static type lint.
The record stays proposed until the first hardware scan completes through the core.
