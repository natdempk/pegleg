#!/usr/bin/env python3
"""Extract a PopCap .pak archive (XOR 0xF7 obfuscated)."""
import os, struct, sys

def main(pak, out):
    data = bytes(b ^ 0xF7 for b in open(pak, 'rb').read())
    magic, ver = struct.unpack_from('<II', data, 0)
    assert magic == 0xBAC04AC0, hex(magic)
    p = 8
    entries = []
    while True:
        flags = data[p]; p += 1
        if flags & 0x80:
            break
        n = data[p]; p += 1
        name = data[p:p+n].decode('latin1'); p += n
        size, = struct.unpack_from('<I', data, p); p += 4
        p += 8  # filetime
        entries.append((name, size))
    for name, size in entries:
        dst = os.path.join(out, name.replace('\\', '/'))
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        with open(dst, 'wb') as f:
            f.write(data[p:p+size])
        p += size
    print(f'{len(entries)} files extracted')

if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
