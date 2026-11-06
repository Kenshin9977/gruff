"""Exploration helper: find a string in an image and the pointers to it, print the bytes around.

    py -3.11 -I probe.py <exe> <string> [...]
"""
import struct
import sys

sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
from image import Image  # noqa: E402


def find_all(mem, needle):
    out, i = [], mem.find(needle)
    while i >= 0:
        out.append(i)
        i = mem.find(needle, i + 1)
    return out


def main():
    im = Image(sys.argv[1])
    print(im.describe())
    for s in sys.argv[2:]:
        hits = find_all(im.mem, b"\0" + s.encode() + b"\0")
        print("string %r: %d hits" % (s, len(hits)))
        for h in hits[:4]:
            va = im.base + h + 1
            pat = struct.pack("<Q" if im.x64 else "<I", va)
            refs = find_all(im.mem, pat)
            print("  at 0x%x, %d pointer refs" % (va, len(refs)))
            for r in refs[:4]:
                w = im.ptr
                row = [im.uptr(im.base + r + k * w) for k in range(-3, 9)]
                print("    ref 0x%x:" % (im.base + r), " ".join("%x" % x for x in row))


if __name__ == "__main__":
    main()
