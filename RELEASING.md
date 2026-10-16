# Publishing caexfer

Publishing is a manual step because a registry version cannot be replaced.
Do not tag a candidate as published until the registry confirms the upload.

1. Confirm the working tree is clean and `main` contains the intended release.
2. Confirm CI and Coverage passed for that exact commit; Coverage must report at
   least 98% distinct Rust source-line coverage.
3. Run `cargo publish --dry-run` without `--allow-dirty`. Check the packaged
   file list with `cargo package --list` and inspect the README, license files,
   source files, and fixture provenance included in the archive.
4. Prepare version-specific GitHub Release notes; for the 0.2.0 candidate, use
   [the draft](docs/RELEASE_NOTES_0.2.0.md). Confirm that the planned version
   is not already published and that the publishing account has a verified
   email and a suitable token.
5. Publish with `cargo publish`, then independently confirm that the version
   appears on crates.io and docs.rs. If the upload times out, inspect
   the registry before retrying.
6. Tag the published commit `v<VERSION>` and push the tag only after confirmation.

The [support contract](docs/SUPPORT.md) defines the release's format scope.
Use [GitHub Releases](https://github.com/cmccomb/caexfer/releases) for version
history. The repository does not maintain a changelog file.
