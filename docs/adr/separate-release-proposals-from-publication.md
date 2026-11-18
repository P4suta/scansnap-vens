---
date: 2026-11-19
status: proposed
---

# Separate release proposals from publication

## Context

The first release tagged five scanner packages with one version.
The source release requires users to build the commands with Rust.

## Decision

Use release-plz in Git-only mode to propose one workspace version and changelog from the published tag.
Build the source and native archives from a selected commit on protected main.
The publication decision covers the signed tag and the complete GitHub draft.

## Alternatives

Release Please can propose a GitHub release.
Its extra file configuration would need to maintain the Cargo lock entries.
Manual version updates already work and repeat the synchronization each time.

## Consequences

The release operator verifies one candidate before publishing.
Each native target adds a build and installation check.
Reconsider the package boundary if a library needs its own registry release.

## Confirmation

Require the pinned proposal to update all five versions and resolve with the locked dependency set.
Unpack each candidate archive and run both commands.
