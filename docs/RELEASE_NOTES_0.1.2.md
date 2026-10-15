# caexfer 0.1.2 release notes

This maintenance update simplifies internal conversion state without changing
the public API or supported formats. Staged output now returns its conversion
report directly after installation, removing placeholder reports and impossible
"missing report" errors. OP2 synthetic-zero provenance comes from its typed
assumption notice instead of a second flag in the internal read result.
Companion mesh preparation no longer clones result field values it immediately
discards. Destination writing also avoids temporary copies of VTU field names
and OP2 displacement matches.

The INP reader now rejects empty comma-separated node and element fields
instead of shifting later values into the missing position. Unsupported options
on `*NODE` and `*ELEMENT` now fail explicitly, including external `INPUT=`
records and named selections that were previously ignored.

The test suite adds malformed BDF, INP, MSH, and OP2 cases; shared mesh
identity checks; and Exodus series and geometry rejection cases. The CLI
conversion matrix now checks STL, SU2, UNV, and Exodus sources and direct UNV
and Exodus output routes. Additional parser, public API, and interrupted-write
tests now cover malformed records, output failures across all destinations,
and individual streaming write calls. A deterministic single-byte corruption
corpus checks that all twelve input
families return normally through public validation. Local measured Rust line
coverage rose from 90.17% on v0.1.1 to 95.01% on the v0.1.2 candidate. The
existing 90% release floor still applies.

Requires Rust 1.86 or newer.
