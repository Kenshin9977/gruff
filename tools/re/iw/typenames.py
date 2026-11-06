"""The VM's value-type names, from the build's own table (var_typename[]: "undefined", "object",
"string", "localized string", "vector", "float", "int", ...): an array of string pointers whose
first two entries are "undefined" and "object". Index = the type id the accessors compare.
"""
import struct


def find(im):
    for first in (b"\0undefined\0",):
        i = im.mem.find(first)
        while i >= 0:
            va = im.base + i + 1
            pat = struct.pack("<Q" if im.x64 else "<I", va)
            j = im.mem.find(pat)
            while j >= 0:
                names = []
                for k in range(40):
                    p = im.uptr(im.base + j + k * im.ptr)
                    s = im.cstr(p, 40) if im.in_image(p) else None
                    if not s or not all(32 <= c < 127 for c in s):
                        break
                    names.append(s.decode())
                if len(names) >= 8 and names[1] == "object":
                    return im.base + j, names
                j = im.mem.find(pat, j + 1)
            i = im.mem.find(first, i + 1)
    return None, []
