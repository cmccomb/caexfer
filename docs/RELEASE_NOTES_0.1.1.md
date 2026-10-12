# caexfer 0.1.1 release notes (draft)

This stabilization update improves recovery when an OP2 file and companion mesh
cannot both be installed. If the second installation fails, the library retains
its completed companion output and reports its path. The CLI retains the staged
companion and, for `--overwrite`, hard links to previous output files in private
recovery directories. The error lists the retained paths for inspection.
Paired installation is still not atomic, and concurrent in-place writes can
change a hard-linked previous file.

The release also clarifies the publishing procedure and the support contract.
It rejects malformed NetCDF-3 header counts before the Exodus dependency can
allocate from them, and adds an MSC Nastran plate fixture checked against an
independent reader. Format support and the public conversion API remain the
same as 0.1.0.

Requires Rust 1.86 or newer.
