# PDFCraft integration validation candidate

This directory records the exact PDFCraft development source being tested with
Rust OpenScan. It is not an official PDFCraft release and is not a claim that all
requested hardware, desktop behavior or feature work is complete.

`upstream.txt` pins the original source revision. The patch applies to that source;
`tree.txt` and `source.json` identify the resulting Git tree. CI reconstructs and
checks that tree before building it. The patch omits reverse copies of deleted
trademark artwork. It contains no scan sessions, device identifiers, test signing
keys, private discussion captures or personal settings.

The dedicated integration workflow in this repository leaves PDFCraft's own CI
unchanged. It builds and tests the native application, interface, engine and
automation crates on Windows, Linux and macOS. It waits for the pinned Rust scanner
build to finish successfully, verifies its source revision, records the downloaded
binary hashes and checks release startup. It then runs a real process-protocol
preset round trip without requiring a physical scanner. Both Linux binaries use
the Ubuntu 24.04 baseline. Linux also checks the WebAssembly build.

Native desktop interaction, real scanner acquisition and device-specific behavior
remain separate checks. See the reconstructed source's `docs/local-integrations.md`
for imported upstream contributions and the standalone scanner's
[`VALIDATION.md`](../../rust/docs/VALIDATION.md) for its precise test coverage.
