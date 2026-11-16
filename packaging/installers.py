"""Create versioned installers from verified OpenScan/PDFCraft portable packages.

Build tools only: Python 3.12+, NSIS (Windows), dpkg-deb and bomutils (Linux).
None of these tools is an application runtime dependency.
"""
from pathlib import Path
import argparse
import gzip
import hashlib
import json
import math
import os
import plistlib
import shutil
import stat
import struct
import subprocess
import tarfile
import time
import xml.etree.ElementTree as ET
import zipfile
import zlib


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify_package(root):
    count = 0
    for line in (root / 'SHA256SUMS.txt').read_text(encoding='utf-8').splitlines():
        expected, name = line.split('  ', 1)
        path = (root / name).resolve(strict=True)
        if not path.is_relative_to(root.resolve()) or digest(path) != expected:
            raise ValueError(f'Package checksum mismatch: {name}')
        count += 1
    return count


def nsis_quote(value):
    return str(value).replace('$', '$$').replace('"', '$\\"')


def windows(args, info):
    root = args.package.resolve()
    out = args.output / (root.name + '-Setup.exe')
    if out.exists():
        raise FileExistsError(out)
    work = args.work
    work.mkdir(parents=True, exist_ok=True)
    source = work / (root.name + '.nsi')
    exe = 'paperdesk.exe' if info['paper'] else 'pdfcraft.exe' if info['pdf'] else 'openscan.exe'
    title = info['title']
    key = root.name
    # The portable package may already be running. Include only its attested
    # distribution files, never recovery locks, settings, or user documents.
    files = sorted([root / line.split('  ', 1)[1]
                    for line in (root / 'SHA256SUMS.txt').read_text(encoding='utf-8').splitlines()]
                   + [root / 'SHA256SUMS.txt'])
    copies = '\n'.join('  SetOutPath "$INSTDIR\\' + nsis_quote(str(p.parent.relative_to(root)))
                       + '"\n  File "' + nsis_quote(p) + '"' for p in files)
    # Only packaged files are removed. Additional files, settings, and scans survive.
    deletes = '\n'.join('  Delete "$INSTDIR\\' + nsis_quote(str(p.relative_to(root))) + '"' for p in files)
    dirs = sorted({parent for p in files for parent in p.parents
                   if parent != root and parent.is_relative_to(root)},
                  key=lambda p: len(p.parts), reverse=True)
    rmdirs = '\n'.join('  RMDir "$INSTDIR\\' + nsis_quote(str(p.relative_to(root))) + '"' for p in dirs)
    source.write_text(f'''Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
Name "{title}"
OutFile "{nsis_quote(out)}"
InstallDir "$LOCALAPPDATA\\Programs\\{info['name']}\\{key}"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "{info['numeric']}"
VIAddVersionKey "ProductName" "{title}"
VIAddVersionKey "FileDescription" "{title} installer"
VIAddVersionKey "FileVersion" "{info['numeric']}"
Var Integrate
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\\{exe}"
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Function .onInit
  SetShellVarContext current
  StrCpy $Integrate 1
  ${{GetParameters}} $R0
  ClearErrors
  ${{GetOptions}} $R0 "/NOINTEGRATION" $R1
  IfErrors +2 0
  StrCpy $Integrate 0
FunctionEnd
Section "Application"
  SetOutPath "$INSTDIR"
{copies}
  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  StrCmp $Integrate 0 no_integration
  CreateDirectory "$SMPROGRAMS\\{key}"
  CreateShortcut "$SMPROGRAMS\\{key}\\{info['name']}.lnk" "$INSTDIR\\{exe}"
  CreateShortcut "$SMPROGRAMS\\{key}\\Read me.lnk" "$INSTDIR\\START-HERE.txt"
  CreateShortcut "$SMPROGRAMS\\{key}\\Uninstall.lnk" "$INSTDIR\\Uninstall.exe"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}" "DisplayName" "{title}"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}" "DisplayVersion" "{info['numeric']}"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}" "UninstallString" '$\\"$INSTDIR\\Uninstall.exe$\\"'
  WriteRegDWORD HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}" "NoModify" 1
  WriteRegDWORD HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}" "NoRepair" 1
  no_integration:
SectionEnd
Section "Uninstall"
  SetShellVarContext current
{deletes}
  Delete "$INSTDIR\\Uninstall.exe"
{rmdirs}
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\\{key}\\{info['name']}.lnk"
  Delete "$SMPROGRAMS\\{key}\\Read me.lnk"
  Delete "$SMPROGRAMS\\{key}\\Uninstall.lnk"
  RMDir "$SMPROGRAMS\\{key}"
  DeleteRegKey HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}"
SectionEnd
''', encoding='utf-8', newline='\n')
    subprocess.run([str(args.makensis), '/V2', str(source)], check=True)
    return out


def linux(args, info):
    root = args.package.resolve()
    stage = args.work / (root.name + '-deb-root')
    stage.mkdir(parents=True, exist_ok=False)
    name = 'paperdesk-openscan-convert2txt-r4' if info['paper'] else 'pdfcraft-openscan-soft3d-r4' if info['pdf'] else 'openscan-soft3d-r4'
    version = '0.3.0+openscan0.3.1.r4' if info['paper'] else '0.21.1+openscan0.3.1.r4' if info['pdf'] else '0.3.1+soft3d.r4'
    appdir = stage / 'opt' / name
    shutil.copytree(root, appdir)
    bindir = stage / 'usr/bin'
    bindir.mkdir(parents=True)
    exe = 'paperdesk' if info['paper'] else 'pdfcraft' if info['pdf'] else 'openscan'
    models = 'OpenScanModels' if info['pdf'] else 'models'
    launch = bindir / name
    isolation = '''export XDG_CONFIG_HOME="$HOME/.config/pdfcraft-openscan"
export XDG_DATA_HOME="$HOME/.local/share/pdfcraft-openscan"
export XDG_CACHE_HOME="$HOME/.cache/pdfcraft-openscan"
''' if info['pdf'] else ''
    launch.write_text('#!/bin/sh\nset -eu\n' + isolation + f'export OPENSCAN_MODELS="/opt/{name}/{models}"\nexec "/opt/{name}/{exe}" ' + ('--new-window ' if info['pdf'] else '') + '"$@"\n')
    launch.chmod(0o755)
    desktopdir = stage / 'usr/share/applications'
    desktopdir.mkdir(parents=True)
    (desktopdir / (name + '.desktop')).write_text(f'''[Desktop Entry]
Type=Application
Name={info['title']}
Exec=/usr/bin/{name} %F
Icon=scanner
Terminal=false
Categories=Office;Graphics;
''')
    for p in stage.rglob('*'):
        p.chmod(0o755 if p.is_dir() or p == launch or p.name in ['openscan', 'paperdesk', 'pdfcraft', 'pdfcraft-cli'] else 0o644)
    control = stage / 'DEBIAN'
    control.mkdir()
    installed_kb = math.ceil(sum(p.stat().st_size for p in stage.rglob('*') if p.is_file()) / 1024)
    (control / 'control').write_text(f'''Package: {name}
Version: {version}
Section: graphics
Priority: optional
Architecture: amd64
Maintainer: kevin7du-cpu <353860035+kevin7du-cpu@users.noreply.github.com>
Installed-Size: {installed_kb}
Depends: libc6 (>= 2.28), libxkbcommon0, sane-utils
Recommends: libgl1, libegl1
Homepage: https://github.com/kevin7du-cpu/openscan
Description: {info['title']} with offline OCR and scanner support
 Rust desktop application with the reusable OpenScan scanner module.
 Requires a compatible scanner driver. This version is installed separately
 from older releases. User scans and documents are retained on removal.
''')
    (control / 'control').chmod(0o644)
    control.chmod(0o755)
    out = args.output / f'{name}_{version}_amd64.deb'
    if out.exists():
        raise FileExistsError(out)
    subprocess.run(['dpkg-deb', '--root-owner-group', '-Zgzip', '--build', str(stage), str(out)], check=True)
    subprocess.run(['dpkg-deb', '--info', str(out)], check=True)
    return out


def write_cpio(root, destination):
    """Write the POSIX odc cpio format used by flat macOS packages."""
    paths = [root, *sorted(root.rglob('*'))]
    with destination.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0) as stream:
        for inode, path in enumerate([*paths, None], 1):
            name = ('TRAILER!!!' if path is None else '.' if path == root else './' + path.relative_to(root).as_posix()).encode() + b'\0'
            mode = 0 if path is None else path.stat().st_mode
            size = path.stat().st_size if path is not None and path.is_file() else 0
            mtime = 0 if path is None else int(path.stat().st_mtime)
            header = '070707' + ''.join(f'{x:06o}' for x in [0, inode, mode, 0, 80, 1, 0]) + f'{mtime:011o}{len(name):06o}{size:011o}'
            assert len(header) == 76
            stream.write(header.encode('ascii'))
            stream.write(name)
            if size:
                with path.open('rb') as source:
                    shutil.copyfileobj(source, stream)
    return paths


def write_xar(files, destination):
    """Write a checksum-protected flat XAR. Payload is already gzip compressed."""
    root = ET.Element('xar')
    toc = ET.SubElement(root, 'toc')
    ET.SubElement(toc, 'creation-time').text = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    checksum = ET.SubElement(toc, 'checksum', style='sha1')
    ET.SubElement(checksum, 'offset').text = '0'
    ET.SubElement(checksum, 'size').text = '20'
    offset = 20
    for i, (name, path) in enumerate(files, 1):
        file = ET.SubElement(toc, 'file', id=str(i))
        for key, value in [('name', name), ('type', 'file'), ('mode', '0644'), ('uid', '0'), ('gid', '80')]:
            ET.SubElement(file, key).text = value
        data = ET.SubElement(file, 'data')
        size = path.stat().st_size
        for key, value in [('length', size), ('offset', offset), ('size', size)]:
            ET.SubElement(data, key).text = str(value)
        ET.SubElement(data, 'encoding', style='application/octet-stream')
        with path.open('rb') as stream:
            sha = hashlib.file_digest(stream, 'sha1').hexdigest()
        ET.SubElement(data, 'archived-checksum', style='sha1').text = sha
        ET.SubElement(data, 'extracted-checksum', style='sha1').text = sha
        offset += size
    xml = ET.tostring(root, encoding='utf-8', xml_declaration=True)
    compressed = zlib.compress(xml)
    with destination.open('xb') as stream:
        stream.write(struct.pack('>IHHQQI', 0x78617221, 28, 1, len(compressed), len(xml), 1))
        stream.write(compressed)
        stream.write(hashlib.sha1(compressed).digest())
        for _, path in files:
            with path.open('rb') as source:
                shutil.copyfileobj(source, stream)


def macos(args, info):
    stage = args.work / (args.package.name + '-pkg')
    stage.mkdir(parents=True, exist_ok=False)
    root = stage / 'root'
    root.mkdir()
    root.chmod(0o755)
    app_name = ('PaperDesk 0.3.0 OpenScan Convert2txt.app' if info['paper'] else 'PDFCraft 0.21.1 OpenScan 0.3.1 Soft3D.app' if info['pdf'] else 'OpenScan 0.3.1 Soft3D.app')
    source = args.package / ('PaperDesk.app' if info['paper'] else 'PdfCraft.app' if info['pdf'] else 'OpenScan.app')
    target = root / 'Applications' / app_name
    shutil.copytree(source, target)
    # Preserve app signatures by changing only filesystem permissions, not bytes.
    for path in root.rglob('*'):
        executable = path.is_file() and path.parent.name == 'MacOS'
        path.chmod(0o755 if path.is_dir() or executable else 0o644)
    metadata = plistlib.loads((target / 'Contents/Info.plist').read_bytes())
    identifier = metadata['CFBundleIdentifier'] + '.soft3d.r4'
    version = metadata['CFBundleVersion']
    pkg = ET.Element('pkg-info', {'format-version': '2', 'identifier': identifier, 'version': version,
                                 'install-location': '/', 'auth': 'root'})
    paths = write_cpio(root, stage / 'Payload')
    size = math.ceil(sum(p.stat().st_size for p in paths if p.is_file()) / 1024)
    ET.SubElement(pkg, 'payload', installKBytes=str(size), numberOfFiles=str(len(paths)))
    bundles = ET.SubElement(pkg, 'bundle-version')
    ET.SubElement(bundles, 'bundle', {'id': metadata['CFBundleIdentifier'],
                                    'CFBundleIdentifier': metadata['CFBundleIdentifier'],
                                    'path': './Applications/' + app_name, 'CFBundleVersion': version,
                                    'CFBundleShortVersionString': metadata['CFBundleShortVersionString']})
    (stage / 'PackageInfo').write_bytes(ET.tostring(pkg, encoding='utf-8', xml_declaration=True))
    subprocess.run([str(args.mkbom), '-u', '0', '-g', '80', str(root), str(stage / 'Bom')], check=True)
    out = args.output / (args.package.name + '-Installer.pkg')
    write_xar([(name, stage / name) for name in ['Bom', 'PackageInfo', 'Payload']], out)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--platform', choices=['windows', 'linux', 'macos'], required=True)
    parser.add_argument('--makensis', type=Path)
    parser.add_argument('--mkbom', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    args.work.mkdir(parents=True, exist_ok=True)
    count = verify_package(args.package)
    pdf = args.package.name.startswith('PDFCraft-')
    paper = args.package.name.startswith('PaperDesk-')
    info = {'pdf': pdf, 'paper': paper, 'name': 'PDFCraft OpenScan' if pdf else 'OpenScan',
            'title': 'PDFCraft 0.21.1 with OpenScan 0.3.1 Soft 3D Light' if pdf else 'OpenScan 0.3.1 Soft 3D Light',
            'numeric': '0.21.1.4' if pdf else '0.3.1.4'}
    if paper:
        info.update(name='PaperDesk', title='PaperDesk 0.3.0 with OpenScan + Convert2txt', numeric='0.3.0.4')
    result = {'windows': windows, 'linux': linux, 'macos': macos}[args.platform](args, info)
    result.with_name(result.name + '.sha256').write_text(f'{digest(result)}  {result.name}\n')
    print(json.dumps({'installer': str(result), 'sha256': digest(result), 'verified_input_files': count}))


if __name__ == '__main__':
    main()
