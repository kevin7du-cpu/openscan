"""Package prebuilt OpenScan 0.3.1 r4 binaries, including a universal macOS app."""
from pathlib import Path
import argparse
import hashlib
import json
import plistlib
import shutil
import struct
import subprocess
import tarfile
import zipfile
from verify_macos import verify_app


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--resources', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--platform', choices=['windows-x64', 'linux-x64', 'macos-universal'], required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--rcodesign', type=Path)
    args = parser.parse_args()
    root = args.output / ('OpenScan-0.3.1-Soft3D-Light-r4-' + args.platform)
    root.mkdir(parents=True, exist_ok=False)
    mac, windows = args.platform == 'macos-universal', args.platform == 'windows-x64'
    app = root / 'OpenScan.app'
    bindir = app / 'Contents/MacOS' if mac else root
    resources = app / 'Contents/Resources' if mac else root
    bindir.mkdir(parents=True, exist_ok=True)
    resources.mkdir(parents=True, exist_ok=True)
    binary = 'openscan.exe' if windows else 'openscan'
    shutil.copyfile(args.binaries / binary, bindir / binary)
    header = (bindir / binary).read_bytes()[:8]
    if mac:
        assert struct.unpack('>II', header) == (0xCAFEBABE, 2), 'Expected universal Intel/Apple Silicon binary'
    else:
        assert header.startswith(b'MZ' if windows else b'\x7fELF'), 'Incorrect executable format'
    model_dir = resources if mac else bindir
    for folder, target in [('models', model_dir), ('licenses', resources)]:
        shutil.copytree(args.resources / folder, target / folder)
    expected_models = {
        'text-detection.rten': 'f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca',
        'text-recognition.rten': 'e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e',
    }
    for name, expected in expected_models.items():
        assert digest(model_dir / 'models' / name) == expected, 'OCR model checksum mismatch'
    # Use current license notices, including the accepted Inter Regular face.
    for name in ['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD_PARTY_NOTICES.md', 'DEPENDENCY_LICENSES.md']:
        if (args.repo / name).is_file():
            shutil.copyfile(args.repo / name, resources / 'licenses' / name)
    for source, destination in [('README.md', 'README.md'),
                                ('docs/OPENSCAN_0_3_1_SOFT_3D.txt', 'SOFT-3D-LIGHT.txt'),
                                ('docs/OPENSCAN_0_3_1_VALIDATION.txt', 'VALIDATION.txt'),
                                ('docs/PaperDesk-Soft-3D-Light-Theme-Guide.txt', 'THEME-GUIDE.txt')]:
        shutil.copyfile(args.repo / source, resources / destination)
    (resources / 'BUILD-SOURCE.json').write_text(json.dumps({
        'application': 'OpenScan', 'version': '0.3.1', 'build': 'Soft3D-Light-r4',
        'source_tree': args.revision, 'platform': args.platform,
        'source_files': {p.relative_to(args.repo).as_posix(): digest(p) for p in
                         [args.repo / 'Cargo.lock', *sorted((args.repo / 'crates/openscan-egui/src').rglob('*.rs'))]},
    }, indent=2) + '\n', encoding='utf-8')
    platform_notes = {
        'windows-x64': 'Run openscan.exe. Install the scanner manufacturer\'s WIA driver.',
        'linux-x64': 'Requires glibc 2.28+, a graphical desktop and SANE scanimage (Ubuntu: sane-utils).\nRun ./install.sh for a per-user installation; older versions are preserved.',
        'macos-universal': 'macOS 12+; Intel and Apple Silicon. Drag OpenScan.app into Applications.\nInstall SANE scanimage separately (for example brew install sane-backends) and a compatible scanner driver.\nThis build uses SANE, not Apple ImageCaptureCore. The app is ad-hoc signed,\nnot Developer ID signed or notarized. Native macOS testing is pending.',
    }[args.platform]
    (root / 'START-HERE.txt').write_text(f'''{root.name}

{platform_notes}
Keep the included OCR models and licenses with the application.
No C# or .NET runtime is required. OCR runs locally.
Opening Scan automatically discovers scanners in the background.
Scan more Pages appends to the holding workspace; Done finishes the batch.
Defaults: black and white, 300 DPI, deskew/despeckle/crop enabled and low
crease/wrinkle cleanup. Controls remain 110%; holding window initially 65%.
Existing document data is preserved. See SOFT-3D-LIGHT.txt for reuse details.
''', encoding='utf-8', newline='\n')
    if mac:
        info = {'CFBundleExecutable': binary, 'CFBundleName': 'OpenScan',
                'CFBundleDisplayName': 'OpenScan', 'CFBundleIdentifier': 'io.github.kevin7du-cpu.openscan',
                'CFBundlePackageType': 'APPL', 'CFBundleShortVersionString': '0.3.1',
                'CFBundleVersion': '0.3.1.4', 'LSMinimumSystemVersion': '12.0',
                'NSHighResolutionCapable': True}
        (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
        (app / 'Contents/PkgInfo').write_bytes(b'APPL????')
        if not args.rcodesign:
            raise ValueError('--rcodesign is required for the macOS app')
        config = root / 'empty-signing-config.toml'
        config.write_text('', encoding='utf-8')
        subprocess.run([str(args.rcodesign), 'sign', '--config-file', str(config), '--timestamp-url', 'none', str(app)], check=True)
        (root / 'MACOS-VERIFICATION.json').write_text(json.dumps(verify_app(app), indent=2)+'\n', encoding='utf-8')
    elif not windows:
        shutil.copyfile(Path(__file__).with_name('install-stock.sh'), root / 'install.sh')
    executable_paths = { (bindir / binary).relative_to(root).as_posix() }
    if not windows and not mac:
        executable_paths.add('install.sh')
    for name in executable_paths:
        (root / name).chmod(0o755)
    files = sorted(p for p in root.rglob('*') if p.is_file())
    (root / 'SHA256SUMS.txt').write_text(''.join(f'{digest(p)}  {p.relative_to(root).as_posix()}\n' for p in files), encoding='utf-8', newline='\n')
    archive = root.with_name(root.name + ('.tar.gz' if args.platform == 'linux-x64' else '.zip'))
    if args.platform == 'linux-x64':
        def permissions(info):
            info.uid = info.gid = 0
            info.uname = info.gname = 'root'
            info.mode = 0o755 if info.isdir() or info.name.removeprefix(root.name + '/') in executable_paths else 0o644
            return info
        with tarfile.open(archive, 'w:gz') as stream:
            stream.add(root, arcname=root.name, filter=permissions)
    else:
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as stream:
            for p in sorted(root.rglob('*')):
                if p.is_file():
                    entry = zipfile.ZipInfo.from_file(p, p.relative_to(root.parent).as_posix())
                    entry.create_system = 3
                    mode = 0o755 if p.relative_to(root).as_posix() in executable_paths else 0o644
                    entry.external_attr = (0o100000 | mode) << 16
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    with p.open('rb') as source, stream.open(entry, 'w') as dest:
                        shutil.copyfileobj(source, dest)
    archive.with_name(archive.name + '.sha256').write_text(f'{digest(archive)}  {archive.name}\n', encoding='utf-8')
    print(json.dumps({'archive': str(archive), 'sha256': digest(archive)}))


if __name__ == '__main__':
    main()
