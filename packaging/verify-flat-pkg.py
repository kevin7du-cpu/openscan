"""Independently read flat macOS packages with the system libarchive library."""
from pathlib import Path
import ctypes as C
import hashlib
import json
import subprocess
import sys
import xml.etree.ElementTree as ET

lib=C.CDLL('libarchive.so.13')
def signature(name,result,args):
    f=getattr(lib,name); f.restype=result; f.argtypes=args; return f
new=signature('archive_read_new',C.c_void_p,[])
filters=signature('archive_read_support_filter_all',C.c_int,[C.c_void_p])
formats=signature('archive_read_support_format_all',C.c_int,[C.c_void_p])
open_file=signature('archive_read_open_filename',C.c_int,[C.c_void_p,C.c_char_p,C.c_size_t])
next_entry=signature('archive_read_next_header',C.c_int,[C.c_void_p,C.POINTER(C.c_void_p)])
pathname=signature('archive_entry_pathname',C.c_char_p,[C.c_void_p])
mode=signature('archive_entry_mode',C.c_uint,[C.c_void_p])
uid=signature('archive_entry_uid',C.c_longlong,[C.c_void_p])
gid=signature('archive_entry_gid',C.c_longlong,[C.c_void_p])
read=signature('archive_read_data',C.c_ssize_t,[C.c_void_p,C.c_void_p,C.c_size_t])
error=signature('archive_error_string',C.c_char_p,[C.c_void_p])
free=signature('archive_read_free',C.c_int,[C.c_void_p])

def entries(path):
    reader=new(); filters(reader); formats(reader)
    if open_file(reader,str(path).encode(),65536)!=0: raise ValueError(error(reader))
    try:
        while True:
            entry=C.c_void_p()
            code=next_entry(reader,C.byref(entry))
            if code==1: break
            if code!=0: raise ValueError(error(reader))
            chunks=[]
            while True:
                buffer=C.create_string_buffer(1048576)
                size=read(reader,buffer,len(buffer))
                if size<0: raise ValueError(error(reader))
                if size==0: break
                chunks.append(buffer.raw[:size])
            yield pathname(entry).decode(),mode(entry),uid(entry),gid(entry),b''.join(chunks)
    finally: free(reader)

pkg=Path(sys.argv[1])
stage=Path(sys.argv[2])
lsbom=Path(sys.argv[3])
audit=stage/'independent-audit'
audit.mkdir(exist_ok=True)
container={}
for name,permissions,owner,group,data in entries(pkg):
    if name not in ['Bom','PackageInfo','Payload'] or name in container:
        raise ValueError('Unexpected flat package member: '+name)
    if data!=(stage/name).read_bytes(): raise ValueError('XAR member differs: '+name)
    (audit/name).write_bytes(data)
    container[name]=hashlib.sha256(data).hexdigest()
if len(container)!=3: raise ValueError('Missing flat package member')
root=(stage/'root').resolve()
expected={'.',*('./'+p.relative_to(root).as_posix() for p in root.rglob('*'))}
seen=set(); count=0
for name,permissions,owner,group,data in entries(audit/'Payload'):
    name=name.rstrip('/')
    if name not in expected or name in seen: raise ValueError('Unexpected payload entry: '+name)
    seen.add(name)
    path=root if name=='.' else root/name.removeprefix('./')
    if not path.resolve().is_relative_to(root): raise ValueError('Payload escapes install root')
    if (owner,group)!=(0,80): raise ValueError('Unexpected payload ownership')
    if path.is_file():
        if hashlib.sha256(data).digest()!=hashlib.sha256(path.read_bytes()).digest(): raise ValueError('Payload bytes differ: '+name)
        if permissions & 0o777 != (0o755 if path.parent.name=='MacOS' else 0o644): raise ValueError('Bad payload permissions: '+name)
        count+=1
    elif permissions & 0o777 != 0o755 or data: raise ValueError('Bad directory: '+name)
if seen!=expected: raise ValueError('Missing payload entries')
info=ET.fromstring((audit/'PackageInfo').read_bytes())
if info.attrib.get('install-location')!='/': raise ValueError('Wrong installation root')
if int(info.find('payload').attrib['numberOfFiles'])!=len(seen): raise ValueError('Wrong payload count')
bom=subprocess.run([str(lsbom),'-s',str(audit/'Bom')],capture_output=True,text=True,check=True)
bom_paths=set(line.rstrip('/') for line in bom.stdout.splitlines() if line)
if bom_paths!=seen: raise ValueError('BOM and payload paths differ: '+repr(bom_paths.symmetric_difference(seen)))
result={'installer':pkg.name,'libarchive_xar':'pass','libarchive_cpio':'pass','bom_paths':'pass',
        'payload_files':count,'entries':len(seen),'identifier':info.attrib['identifier'],
        'signing':'Ad-hoc application signatures; installer is unsigned; native macOS test pending'}
pkg.with_name(pkg.name+'.verification.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result))
