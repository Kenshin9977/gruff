"""PE image for the IW extraction: common/image.py's Image (x86 or x64, flat or file layout, the
sections placed at their RVA in memory), seen through the names the IW scripts use (`mem`,
`pdata`) plus what only they need: the import directory, entropy, and a `cstr` that returns
bytes up to a limit.
"""
import math
import os
import struct
import sys


sys.path.insert(1, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))  # tools/re
from common import image as _common  # noqa: E402
from common.image import user_site  # noqa: E402,F401  (re-exported: the iw scripts import it from here)


class Image(_common.Image):
    def __init__(self, path):
        super().__init__(path)
        self.mem = self.d        # the image as mapped (offset == RVA)
        self.pdata = self.pd     # .pdata (rva begin, rva end), x64 only

    def imports(self):
        """[(iat slot va, dll, function name or '#ordinal')] from the import directory."""
        d = self.mem
        e = struct.unpack_from("<I", d, 0x3C)[0]
        opt = e + 24
        dd = opt + (112 if self.x64 else 96)
        imp_rva, imp_sz = struct.unpack_from("<II", d, dd + 8)
        out = []
        if not imp_rva:
            return out
        o = imp_rva
        while o + 20 <= self.size:
            oft, _, _, name_rva, ft = struct.unpack_from("<IIIII", d, o)
            if not name_rva:
                break
            dll = (self.cstr(self.base + name_rva) or b"?").decode("latin1")
            lookup = oft or ft
            if not (0 < lookup < self.size and 0 < ft < self.size):
                break  # a dump's rebuilt or damaged import directory: stop at the first bad one
            k = 0
            while lookup + self.ptr * (k + 1) <= self.size:
                if self.x64:
                    v = struct.unpack_from("<Q", d, lookup + 8 * k)[0]
                    by_ord = v >> 63
                else:
                    v = struct.unpack_from("<I", d, lookup + 4 * k)[0]
                    by_ord = v >> 31
                if not v:
                    break
                if by_ord:
                    nm = "#%d" % (v & 0xFFFF)
                else:
                    nm = (self.cstr(self.base + (v & 0x7FFFFFFF) + 2) or b"?").decode("latin1")
                out.append((self.base + ft + self.ptr * k, dll, nm))
                k += 1
            o += 20
        return out

    # --- addresses -----------------------------------------------------------------------
    def va(self, rva):
        return self.base + rva

    def rva(self, va):
        return va - self.base

    def cstr(self, va, limit=256):
        o = va - self.base
        if not 0 <= o < self.size:
            return None
        j = self.mem.find(b"\0", o, o + limit)
        if j < 0:
            return None
        return self.mem[o:j]

    def entropy(self, name):
        for s in self.sections:
            if s[0] == name:
                b = self.mem[s[1]:s[1] + s[2]]
                if not b:
                    return 0.0
                c = [0] * 256
                for x in b[:: max(1, len(b) // 2000000)]:
                    c[x] += 1
                n = sum(c)
                return -sum(v / n * math.log2(v / n) for v in c if v)
        return None

    def describe(self):
        return "%s: %s, %s, base 0x%x, PE timestamp %d, %d sections, %s" % (
            os.path.basename(self.path), "x64" if self.x64 else "x86",
            "flat" if self.flat else "file layout", self.base, self.timestamp,
            len(self.sections), " ".join("%s[0x%x]" % (s[0], s[2]) for s in self.sections))


if __name__ == "__main__":
    for p in sys.argv[1:]:
        im = Image(p)
        print(im.describe())
        print("   entropy", {s[0]: round(im.entropy(s[0]), 2) for s in im.sections if s[2]})
