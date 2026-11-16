Installer tooling for OpenScan 0.3.1 Soft 3D Light r4

package-stock.py assembles already-built Windows x64, Linux x64 or macOS universal
binaries with verified OCR models and license notices. Run --help for explicit
source, resource, output and signer paths. It refuses to overwrite older packages.
installers.py produces per-user NSIS setup EXEs, Debian packages or flat macOS PKGs.
All payloads are checked against their SHA256SUMS.txt before packaging. Windows
uninstall deletes only packaged files and retains extra documents/settings/scans.

Build tools: Python 3.12+, NSIS 3.13; dpkg-deb for Debian; bomutils mkbom for macOS
package metadata (https://github.com/hogliux/bomutils, revision
c247002e2f49878bab32dd0fcf805810ae0992cb). These external tools are not linked into
or shipped as application runtime dependencies. The apps remain native Rust.
Mac bundles are locally ad-hoc signed with rcodesign 0.29.0 and independently checked
for universal architecture, code-page digests and resource seals. They are not
Developer ID signed or notarized; native macOS installer/runtime testing is required.

Independent PKG validation uses libarchive to read XAR and CPIO, compares every
payload byte with the signed bundle and checks BOM paths. The bomutils lsbom
reader at the pinned revision has an inverted Paths-variable test. For validation,
change `if (strstr(name.c_str(),"Paths") == 0)` to `if (name == "Paths")` in
src/lsbom.cpp and rebuild lsbom. This reader-only correction does not change
mkbom or the generated metadata. Retain the utility's GPL-2.0 license.
