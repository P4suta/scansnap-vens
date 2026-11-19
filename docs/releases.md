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

Select the product version from the merged proposal and its full commit SHA.
The build accepts a commit in the history of protected `main`.
Dispatch [Build release candidate](https://github.com/P4suta/scansnap-vens/actions/workflows/build-release-candidate.yml) on `main` with both values.

```sh
gh workflow run build-release-candidate.yml --ref main \
  -f source_sha="$source_sha" -f version="$version"
```

The job checks the locked version and exercises both commands from each archive.
Download the platform artifacts from the same successful run within seven days.
The Windows job runs for sources that include the Windows responder.
It links the Visual C++ runtime statically and checks the extracted x64 executables for Redistributable dependencies.
It extracts the ZIP and exercises discovery and the capabilities request with the archived daemon.
The workflow attests the Windows `*.authenticode-sha256` file with the unsigned ZIP.
The file identifies the source, the unsigned ZIP's checksum, and each executable's Authenticode digest.
The Linux artifact includes the source archive and a CycloneDX inventory.
The build compares the inventory with every tracked Cargo lockfile.
Check each downloaded set with its `SHA256SUMS` file.
Verify each file's [build provenance](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations) for the workflow on `main`.
Compare the selected commit with `SOURCE_COMMIT` inside each binary archive.
Read the source archive's commit from the unpacked tar stream with `git get-tar-commit-id`.

```sh
gh attestation verify "$asset" --repo P4suta/scansnap-vens \
  --signer-workflow P4suta/scansnap-vens/.github/workflows/build-release-candidate.yml \
  --source-ref refs/heads/main
```

## Publish the verified files

Before distributing the macOS archive, sign both executables with Developer ID Application and hardened runtime.
[Submit the signed ZIP to Apple for notarization](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).
After acceptance, check both signatures and execute the commands from the signed distribution on Apple silicon.
Use the final signed ZIP when calculating its release checksum.

For Windows, sign `vens.exe` and `vensd.exe` with [SSL.com eSigner](https://www.ssl.com/guide/esigner-codesigntool-command-guide/) and a timestamp.
Rebuild the ZIP with the signed commands, the license files, and `SOURCE_COMMIT`.
Unpack the rebuilt Windows ZIP.
Compare the license files with the unsigned candidate.
Use [SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool) to confirm the intended signer and a timestamp for each command.
Compare the [Authenticode digests](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/applocker/understanding-the-file-hash-rule-condition-in-applocker) with the attested record to verify the executable content after signing.

In Windows PowerShell, set `$candidateZip` and `$record` to the two attested candidate files.
Set `$sourceSha` to the selected commit and `$signedDir` to the unpacked archive directory.

```powershell
$archiveHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $candidateZip).Hash.ToLowerInvariant()
$expected = @("SOURCE_COMMIT $sourceSha", "UNSIGNED_ZIP_SHA256 $archiveHash")
if ((Get-Content -LiteralPath "$signedDir/SOURCE_COMMIT") -cne $sourceSha) { throw 'Source commit mismatch.' }
foreach ($command in 'vens.exe', 'vensd.exe') {
    $executable = (Resolve-Path -LiteralPath "$signedDir/$command").Path
    signtool verify /all /pa /tw /v $executable
    if ($LASTEXITCODE -ne 0) { throw "Signature verification failed for $command." }
    $digest = (Get-AppLockerFileInformation -Path $executable).Hash
    if ($digest.HashType -ne 'SHA256' -or $digest.HashDataString -notmatch '^0x[0-9a-fA-F]{64}$') { throw "Invalid Authenticode digest for $command." }
    $expected += "$($digest.HashDataString.Substring(2).ToLowerInvariant())  $command"
}
if (((Get-Content -LiteralPath $record) -join "`n") -cne ($expected -join "`n")) { throw 'The signed executable content differs from the candidate.' }
```

Exercise the signed commands from the extracted ZIP and record the checksum of the rebuilt ZIP.
Attach the attested `*.authenticode-sha256` file unchanged with the signed ZIP.

When the candidate is ready for publication, make a signed `vX.Y.Z` tag at the selected commit and verify its signature.
Push the selected tag to GitHub.
Create a GitHub release draft for the existing tag and attach the complete source archive, binaries, dependency inventory, and final `SHA256SUMS`.
Confirm the draft asset hashes match the verified files before publishing.
Download the public files and verify their hashes and the release attestation.
Use [`gh release verify-asset`](https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity) to confirm the downloaded ZIP matches the immutable release asset.

If publication stops before the release goes live, resume the same draft with the original verified files.
For a correction after publication, prepare a new version.
