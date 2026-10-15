# Release candidate checks

These checks apply to each candidate release.

1. Confirm the intended source commit, version, and a clean working tree.
2. Run unit, integration, doctest, documentation, formatting, Clippy, package,
   coverage, and independent interoperability checks. In particular, verify
   injected second-install failures and destination races for both CLI overwrite
   and library no-clobber pairs. Review the recovery files and support-contract
   wording before release.
3. Confirm the supported-platform CI matrix and coverage workflow pass for the
   exact source commit. Publish the crate, then verify its recorded source
   commit before tagging and creating the matching GitHub release.

For 0.2.0, retain the v0.1.2 INP, interrupted-output, conversion-matrix, and
malformed-input regressions. Verify the MSC Nastran plate fixture provenance,
upstream BSD terms, and pyNastran comparison of IDs, topology, and displacement
values. Run the independent VTK/VTI exchange checks, confirm the voxel mesh
limits, and measure distinct Rust source-line coverage above the 95% floor.
Review the draft release notes before publishing.
