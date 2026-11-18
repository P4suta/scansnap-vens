# Prepare and publish a release

## Prepare the version

release-plz compares the five scanner packages with their published version tag.
It updates the shared workspace version, Cargo lockfile, and changelog in a draft pull request.
Review the proposal and merge it when the required checks pass.

The proposal workflow uses the release-plz GitHub App through a read-only Doppler Service Token in the `release-proposal` environment.
Restrict the environment to the protected default branch.
Run [Verify release App access](https://github.com/P4suta/scansnap-vens/actions/workflows/verify-release-app.yml) before setting the repository variable `RELEASE_PROPOSALS_ENABLED` to `true`.
Replace the environment's `DOPPLER_TOKEN` before the Service Token expires, and run the access check again.
Revoke the old Service Token after the access check passes.

## Build the candidate

Select the product version from the merged proposal and the full commit SHA from `main`.
Dispatch [Build release candidate](https://github.com/P4suta/scansnap-vens/actions/workflows/build-release-candidate.yml) on `main` with both values.

```sh
gh workflow run build-release-candidate.yml --ref main \
  -f source_sha="$source_sha" -f version="$version"
```

The job checks the locked version and exercises both commands from each archive.
Download the Linux and macOS artifacts from the same successful run within seven days.
The Linux artifact includes the source archive and a CycloneDX inventory of the locked dependencies.
Check each downloaded set with its `SHA256SUMS` file.
Verify that each file's [build provenance](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations) names the workflow and selected commit.

```sh
gh attestation verify "$asset" --repo P4suta/scansnap-vens \
  --signer-workflow P4suta/scansnap-vens/.github/workflows/build-release-candidate.yml \
  --source-digest "$source_sha"
```

## Publish the verified files

Before distributing the macOS archive, sign both executables with Developer ID Application and hardened runtime.
[Submit the signed ZIP to Apple for notarization](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).
After acceptance, check both signatures and execute the commands from the signed distribution on Apple silicon.
Use the final signed ZIP when calculating its release checksum.

When the candidate is ready for publication, make a signed `vX.Y.Z` tag at the selected commit and verify its signature.
Push the selected tag to GitHub.
Create a GitHub release draft for the existing tag and attach the complete source archive, binaries, dependency inventory, and final `SHA256SUMS`.
Confirm the draft asset hashes match the verified files before publishing.
Download the public files and verify their hashes and the release attestation.

If publication stops before the release goes live, resume the same draft with the original verified files.
For a correction after publication, prepare a new version.
