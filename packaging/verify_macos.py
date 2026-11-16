# SPDX-License-Identifier: MIT OR Apache-2.0
"""Cross-platform inspection of universal Mach-O code pages and bundle resource seals."""
from pathlib import Path
import hashlib
import plistlib
import struct
def sha(data): return hashlib.sha256(data).hexdigest()

def inspect_macho(data: bytes) -> dict:
    if data[:4] != b"\xca\xfe\xba\xbe":
        raise ValueError("Expected a universal Mac application with both architectures")
    count = struct.unpack_from(">I", data, 4)[0]
    slices = []
    for i in range(count):
        cpu, _, offset, size, _ = struct.unpack_from(">IIIII", data, 8 + i * 20)
        part = data[offset:offset + size]
        if part[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError("Expected Mach-O 64 bit slice")
        commands = struct.unpack_from("<I", part, 16)[0]
        pos, libraries, signed, minimum, text_hash = 32, [], False, None, None
        for _ in range(commands):
            cmd, length = struct.unpack_from("<II", part, pos)
            if length < 8 or pos + length > len(part):
                raise ValueError("Invalid Mach-O load command")
            if cmd in (0xC, 0x80000018, 0x8000001F):
                start = struct.unpack_from("<I", part, pos + 8)[0]
                libraries.append(part[pos + start:pos + length].split(b"\0", 1)[0].decode())
            if cmd == 0x1D:
                signed = True
            if cmd == 0x19:  # LC_SEGMENT_64: preserve code bytes across signing.
                section_count = struct.unpack_from("<I", part, pos + 64)[0]
                for section_index in range(section_count):
                    section_pos = pos + 72 + 80 * section_index
                    if part[section_pos:section_pos + 16].rstrip(b"\0") == b"__text":
                        _, text_size, text_offset = struct.unpack_from("<QQI", part, section_pos + 32)
                        text_hash = sha(part[text_offset:text_offset + text_size])
            if cmd == 0x32:
                platform, deployment = struct.unpack_from("<II", part, pos + 8)
                if platform != 1:
                    raise ValueError("Expected macOS platform")
                minimum = f"{deployment >> 16}.{(deployment >> 8) & 255}"
            if cmd == 0x24:
                deployment = struct.unpack_from("<I", part, pos + 8)[0]
                minimum = f"{deployment >> 16}.{(deployment >> 8) & 255}"
            pos += length
        if minimum != "12.0":
            raise ValueError(f"Expected macOS 12.0 minimum, got {minimum}")
        if not libraries or any(not p.startswith(("/System/Library/", "/usr/lib/")) for p in libraries):
            raise ValueError(f"Non-system Mac library dependency: {libraries}")
        if text_hash is None:
            raise ValueError("Missing Mac machine-code section")
        slices.append({"architecture": {0x1000007: "x86_64", 0x100000C: "arm64"}[cpu], "minimum_macos": minimum, "signed": signed, "text_sha256": text_hash, "libraries": libraries})
    if sorted(s["architecture"] for s in slices) != ["arm64", "x86_64"]:
        raise ValueError("Both Intel and Apple Silicon are required")
    return {"format": "universal Mach-O", "slices": slices}


def verify_adhoc(app: Path, data: bytes, identifier: str) -> list[dict]:
    """Check ad-hoc CodeDirectory digests, not Apple identity/notarization.

    Format reference: Apple's Security/libsecurity_codesigning/codedirectory.h.
    rcodesign 0.29's verify incorrectly tries to parse the empty CMS blob of an
    ad-hoc signature. Independently check every code page and special slot here.
    Apple's native codesign/Gatekeeper acceptance remains a separate Mac check.
    """
    results = []
    for index in range(struct.unpack_from(">I", data, 4)[0]):
        cpu, _, offset, size, _ = struct.unpack_from(">IIIII", data, 8 + index * 20)
        part = data[offset:offset + size]
        pos, signature_offset, signature_size = 32, None, None
        for _ in range(struct.unpack_from("<I", part, 16)[0]):
            cmd, length = struct.unpack_from("<II", part, pos)
            if cmd == 0x1D:
                signature_offset, signature_size = struct.unpack_from("<II", part, pos + 8)
            pos += length
        if signature_offset is None or signature_offset + signature_size > len(part):
            raise ValueError("Invalid signature range")
        signature = part[signature_offset:signature_offset + signature_size]
        magic, length, count = struct.unpack_from(">III", signature)
        if magic != 0xFADE0CC0 or length > len(signature):
            raise ValueError("Invalid embedded signature")
        blobs = {}
        for entry in range(count):
            slot, start = struct.unpack_from(">II", signature, 12 + 8 * entry)
            blob_length = struct.unpack_from(">I", signature, start + 4)[0]
            if start + blob_length > length or slot in blobs:
                raise ValueError("Invalid signature blob")
            blobs[slot] = signature[start:start + blob_length]
        directory = blobs[0]
        cd_magic, cd_length, version, flags, hash_offset, ident_offset, special, pages, limit = struct.unpack_from(">9I", directory)
        hash_size, hash_type, _, page_exp = struct.unpack_from("4B", directory, 36)
        if cd_magic != 0xFADE0C02 or cd_length != len(directory) or not flags & 2:
            raise ValueError("Expected an ad-hoc CodeDirectory")
        if version != 0x20400 or hash_size != 32 or hash_type != 2 or page_exp != 12:
            raise ValueError("Unexpected CodeDirectory format; review before shipping")
        if directory[ident_offset:].split(b"\0", 1)[0] != identifier.encode("utf-8"):
            raise ValueError("Wrong signed application identifier")
        if limit != signature_offset or pages != (limit + 4095) // 4096:
            raise ValueError("Signature does not cover the complete executable")
        if hash_offset + pages * 32 != len(directory) or hash_offset - special * 32 < 88:
            raise ValueError("Invalid signature hash table")
        for page in range(pages):
            digest = hashlib.sha256(part[page * 4096:min(limit, (page + 1) * 4096)]).digest()
            if digest != directory[hash_offset + page * 32:hash_offset + (page + 1) * 32]:
                raise ValueError(f"Mac code page digest mismatch: slice {index}, page {page}")
        external = {1: (app / "Contents/Info.plist").read_bytes(), 3: (app / "Contents/_CodeSignature/CodeResources").read_bytes()}
        for slot in range(1, special + 1):
            expected = directory[hash_offset - slot * 32:hash_offset - (slot - 1) * 32]
            content = external.get(slot, blobs.get(slot))
            actual = hashlib.sha256(content).digest() if content is not None else bytes(32)
            if actual != expected:
                raise ValueError(f"Mac signed metadata mismatch: slice {index}, slot {slot}")
        if not {1, 3}.issubset(range(1, special + 1)):
            raise ValueError("Bundle metadata and resources must be sealed")
        results.append({"cpu": cpu, "verified_code_pages": pages, "verified_special_slots": special})
    return results



def verify_app(app):
    info=plistlib.loads((app/'Contents/Info.plist').read_bytes())
    binary=app/'Contents/MacOS'/info['CFBundleExecutable']
    data=binary.read_bytes()
    result=inspect_macho(data)
    result['code_directories']=verify_adhoc(app,data,info['CFBundleIdentifier'])
    seal=plistlib.loads((app/'Contents/_CodeSignature/CodeResources').read_bytes())
    count=0
    for name,value in seal.get('files2',{}).items():
        path=(app/'Contents'/name).resolve(strict=True)
        if not path.is_relative_to((app/'Contents').resolve()): raise ValueError('Resource escapes bundle')
        if isinstance(value,dict) and value.get('hash2'):
            if hashlib.sha256(path.read_bytes()).digest()!=value['hash2']: raise ValueError('Resource seal mismatch: '+name)
            count+=1
    expected=[p for p in (app/'Contents').rglob('*') if p.is_file() and p!=binary and '_CodeSignature' not in p.parts and p.name not in ['Info.plist','PkgInfo']]
    for path in expected:
        relative=path.relative_to(app/'Contents').as_posix()
        if relative not in seal.get('files2',{}): raise ValueError('Unsealed bundle resource: '+relative)
    result['sealed_resources']=count
    # Prove this is not a vacuous verifier using a modified executable in memory.
    tampered=bytearray(data)
    offset=struct.unpack_from('>I',tampered,16)[0]
    tampered[offset+0x10000]^=1
    try: verify_adhoc(app,bytes(tampered),info['CFBundleIdentifier'])
    except ValueError as error:
        if 'code page digest mismatch' not in str(error): raise
    else: raise ValueError('Tampered executable passed integrity verification')
    return result
