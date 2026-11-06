"""PE image for the extractions: x86 or x64, flat (a Cordycep dump flattened: raw offset == RVA)
or a normal file layout (a Steam exe, a dump not flattened). The sections are placed at their RVA
in memory, so `img.d` is always the image as the loader maps it (offset == RVA): no flat copy on
disk is needed.

Addresses taken and returned are virtual addresses (image base + RVA) unless a name says rva.
"""
import bisect
import os
import re
import struct
import sys
import site

SCN_CODE = 0x20000000  # IMAGE_SCN_MEM_EXECUTE


def user_site():
    """Run under `python -I`: add the user site-packages (unicorn, capstone, numpy) explicitly,
    so neither the script directory nor the current directory is ever on sys.path."""
    p = site.getusersitepackages()
    if p not in sys.path:
        sys.path.append(p)


user_site()
import capstone  # noqa: E402


class Image:
    def __init__(self, path):
        self.path = path
        with open(path, "rb") as f:
            raw_file = f.read()
        d = raw_file
        self.file_size = len(d)
        e = struct.unpack_from("<I", d, 0x3C)[0]
        if d[e:e + 4] != b"PE\0\0":
            raise SystemExit("%s: not a PE image" % path)
        self.machine = struct.unpack_from("<H", d, e + 4)[0]
        self.x64 = self.machine == 0x8664
        nsec = struct.unpack_from("<H", d, e + 6)[0]
        self.timestamp = struct.unpack_from("<I", d, e + 8)[0]
        optsz = struct.unpack_from("<H", d, e + 20)[0]
        opt = e + 24
        if self.x64:
            self.base = struct.unpack_from("<Q", d, opt + 24)[0]
            dd = opt + 112
        else:
            self.base = struct.unpack_from("<I", d, opt + 28)[0]
            dd = opt + 96
        self.entry_rva = struct.unpack_from("<I", d, opt + 16)[0]
        hdr = struct.unpack_from("<I", d, opt + 60)[0]
        self.sections = []  # (name, rva, size, characteristics)
        raw = []
        for i in range(nsec):
            o = opt + optsz + 40 * i
            name = d[o:o + 8].rstrip(b"\0").decode("latin1")
            vsz, va, rsz, rptr = struct.unpack_from("<IIII", d, o + 8)
            ch = struct.unpack_from("<I", d, o + 36)[0]
            self.sections.append((name, va, max(vsz, rsz), ch))
            raw.append((va, vsz, rsz, rptr))
        self.flat = all(rp == va for va, _, rs, rp in raw if rs)
        size = max(va + max(vs, rs) for va, vs, rs, _ in raw)
        size = (size + 0xFFF) & ~0xFFF
        if self.flat:
            mem = d[:size] if len(d) >= size else d + bytes(size - len(d))
        else:
            m = bytearray(size)
            m[:hdr] = d[:hdr]
            for va, vs, rs, rp in raw:
                n = min(rs, vs) if vs else rs
                m[va:va + n] = d[rp:rp + n]
            mem = bytes(m)
        del d, raw_file
        self.d = mem
        self.size = size
        self.ptr = 8 if self.x64 else 4
        exc_rva, exc_sz = struct.unpack_from("<II", mem, dd + 8 * 3)
        pd = []
        if self.x64 and exc_sz:
            for o in range(exc_rva, exc_rva + exc_sz, 12):
                b, en, _ = struct.unpack_from("<III", mem, o)
                if b:
                    pd.append((b, en))
        self.pd = sorted(pd)          # .pdata (rva begin, rva end), x64 only
        self.st = [b for b, _ in self.pd]
        self.texts = [(self.base + s[1], self.base + s[1] + s[2]) for s in self.sections
                      if s[3] & SCN_CODE]
        self.md = capstone.Cs(capstone.CS_ARCH_X86,
                              capstone.CS_MODE_64 if self.x64 else capstone.CS_MODE_32)

    # --- identity ------------------------------------------------------------------------------
    def describe(self):
        return "%s: %s, %s, image base %#x, PE timestamp %d, size %d" % (
            os.path.basename(self.path), "x64" if self.x64 else "x86",
            "flat" if self.flat else "file layout", self.base, self.timestamp, self.size)

    # --- addresses -----------------------------------------------------------------------------
    def in_image(self, va):
        return 0 <= va - self.base < self.size

    def section_of(self, va):
        r = va - self.base
        for s in self.sections:
            if s[1] <= r < s[1] + s[2]:
                return s
        return None

    def is_code(self, va):
        return any(a <= va < b for a, b in self.texts)

    def u8(self, va):
        return self.d[va - self.base]

    def u32(self, va):
        return struct.unpack_from("<I", self.d, va - self.base)[0]

    def u64(self, va):
        return struct.unpack_from("<Q", self.d, va - self.base)[0]

    def uptr(self, va):
        return self.u64(va) if self.x64 else self.u32(va)

    def cstr(self, va, n=200):
        """The printable ASCII C string at `va`, or None."""
        o = va - self.base
        if not 0 <= o < self.size:
            return None
        s = self.d[o:o + n].split(b"\0")[0]
        if s and all(32 <= c < 127 for c in s):
            return s.decode("latin1")
        return None

    def find_all(self, needle, start=0, end=None):
        """Every VA where `needle` (bytes) occurs."""
        out, i = [], self.d.find(needle, start)
        end = self.size if end is None else end
        while 0 <= i < end:
            out.append(self.base + i)
            i = self.d.find(needle, i + 1)
        return out

    def find_cstr(self, s):
        """VAs of the NUL-terminated string `s` (str or bytes) that starts after a NUL."""
        b = s.encode("latin1") if isinstance(s, str) else s
        return [v + 1 for v in self.find_all(b"\0" + b + b"\0")]

    def disasm(self, va, n=0x400):
        """The instructions decoded linearly from `va` over `n` bytes."""
        return list(self.md.disasm(self.d[va - self.base:va - self.base + n], va))

    # --- functions -----------------------------------------------------------------------------
    def func(self, va):
        """(start, end) VAs of the .pdata function holding `va` (x64), else None."""
        r = va - self.base
        i = bisect.bisect_right(self.st, r) - 1
        if i >= 0 and self.pd[i][0] <= r < self.pd[i][1]:
            return self.base + self.pd[i][0], self.base + self.pd[i][1]
        return None

    def head_of(self, va):
        """Start of the function holding `va`: .pdata splits some functions into chunks; walk
        back over chunks that fall through into the next (last instruction not ret/jmp/int3)."""
        i = bisect.bisect_right(self.st, va - self.base) - 1
        while i > 0 and self.pd[i - 1][1] == self.pd[i][0]:
            b, e = self.pd[i - 1]
            last = None
            for ins in self.md.disasm(self.d[b:e], self.base + b):
                last = ins
            if last is None or last.mnemonic in ("ret", "jmp", "int3"):
                break
            i -= 1
        return self.base + self.pd[i][0]

    def body(self, va, limit=0x2000):
        """Instructions from `va` to the end of its .pdata function, or (x86, no .pdata) up to
        the first ret / int3 padding, at most `limit` bytes."""
        f = self.func(va)
        if f:
            return list(self.md.disasm(self.d[va - self.base:f[1] - self.base], va))
        out = []
        reach = va  # furthest forward branch target inside the function seen so far
        for ins in self.md.disasm(self.d[va - self.base:va - self.base + limit], va):
            if ins.mnemonic == "int3":
                break
            out.append(ins)
            if ins.mnemonic.startswith("j") and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                if va < t < va + limit:
                    reach = max(reach, t)
            if ins.mnemonic in ("ret", "retn") or (ins.mnemonic == "jmp" and not
                                                   ins.op_str.startswith("0x")):
                if reach <= ins.address:
                    break
        return out

    def end_of(self, va, limit=0x2000):
        f = self.func(va)
        if f:
            return f[1]
        b = self.body(va, limit)
        return b[-1].address + b[-1].size if b else va

    # --- references ----------------------------------------------------------------------------
    def operand_addr(self, ins):
        """The absolute address an instruction's memory or immediate operand names: rip+disp
        (x64), or a 32-bit absolute (x86 disp32 / imm32 inside the image). None otherwise."""
        m = RIP_REL.search(ins.op_str)
        if m:
            off = int(m.group(2), 16)
            return ins.address + ins.size + (off if m.group(1) == "+" else -off)
        if not self.x64:
            for m in HEX.finditer(ins.op_str):
                v = int(m.group(0), 16)
                if self.in_image(v):
                    return v
        return None

    def strings_loaded(self, va, depth=1, seen=None):
        """C strings a function references (lea rip+X on x64; push/mov/lea of an absolute on
        x86), and those of its direct callees down to `depth`."""
        seen = seen if seen is not None else set()
        if va in seen or not self.in_image(va):
            return []
        seen.add(va)
        out, callees = [], []
        for i in self.body(va):
            if i.mnemonic in ("lea", "push", "mov"):
                a = self.operand_addr(i)
                if a is not None:
                    s = self.cstr(a)
                    if s:
                        out.append(s)
            if i.mnemonic == "call" and i.op_str.startswith("0x"):
                callees.append(int(i.op_str, 16))
        if depth:
            for c in callees:
                out += self.strings_loaded(c, depth - 1, seen)
        return out

    def direct_calls(self, va, jumps=True):
        """Targets of the direct calls (and jmps outside the function) in the function at va."""
        end = self.end_of(va)
        out = []
        for ins in self.body(va):
            if ins.mnemonic == "call" or (jumps and ins.mnemonic == "jmp"):
                if ins.op_str.startswith("0x"):
                    t = int(ins.op_str, 16)
                    if ins.mnemonic == "call" or not (va <= t < end):
                        out.append(t)
        return out

    def riprel_xrefs(self, targets):
        """Every rip-relative disp32 in code whose target is in `targets` (VAs): {target: [va of
        the disp32]}. Instruction boundaries are not decoded: callers confirm with capstone."""
        np = _np()
        out = {t: [] for t in targets}
        tset = np.array(sorted(targets), dtype=np.int64)
        for sva, vsz in code_sections(self.sections):
            b = section_bytes(self.d, sva, vsz - 4)
            n = len(b) - 3
            tgt = self.base + sva + np.arange(n, dtype=np.int64) + 4 + le_s32(b, 0, n)
            for i in np.nonzero(np.isin(tgt, tset))[0]:
                out[int(tgt[i])].append(self.base + sva + int(i))
        return out

    def abs_refs(self, targets, code_only=False):
        """Every aligned-or-not 4-byte (x86) / 8-byte (x64) little-endian value equal to one of
        `targets`: pointers in data tables, imm32/disp32 operands in x86 code. {target: [va]}."""
        np = _np()
        out = {t: [] for t in targets}
        tset = np.array(sorted(targets), dtype=np.uint64)
        w = self.ptr
        for name, sva, vsz, ch in self.sections:
            if code_only and not ch & SCN_CODE:
                continue
            b = section_bytes(self.d, sva, vsz, np.uint64)
            n = len(b) - (w - 1)
            v = np.zeros(n, dtype=np.uint64)
            for k in range(w):
                v |= b[k:k + n] << np.uint64(8 * k)
            for i in np.nonzero(np.isin(v, tset))[0]:
                out[int(v[i])].append(self.base + sva + int(i))
        return out

    def rel32_callers(self, targets):
        """Every E8 (call) / E9 (jmp) rel32 in code landing on one of `targets`: {target: [(va,
        "call"|"jmp")]}."""
        np = _np()
        out = {t: [] for t in targets}
        tset = np.array(sorted(targets), dtype=np.int64)
        for sva, _, op, tgt in rel32_branches(self.d, self.base, self.sections):
            hit = np.isin(tgt, tset) & ((op == 0xE8) | (op == 0xE9))
            for i in np.nonzero(hit)[0]:
                out[int(tgt[i])].append((self.base + sva + int(i),
                                         "call" if op[i] == 0xE8 else "jmp"))
        return out

    def code_refs(self, target):
        """VAs of the instructions (decoded, so confirmed) that reference `target`: rip-relative
        on x64, an absolute operand on x86."""
        return self.code_refs_many({target})[target]

    def code_refs_many(self, targets):
        """code_refs for several targets in one scan: {target: [instruction VAs]}."""
        targets = set(targets)
        cands = (self.riprel_xrefs(targets) if self.x64 else
                 self.abs_refs(targets, code_only=True))
        res = {}
        for target, cand in cands.items():
            out = []
            for c in cand:
                va = self._insn_holding(c, target)
                if va is not None:
                    out.append(va)
            res[target] = sorted(set(out))
        return res

    def _insn_holding(self, c, target):
        """Start of the instruction whose operand bytes at `c` name `target`. Decoded from the
        start of its .pdata function when it has one, so the boundary is the code's own and a
        REX or legacy prefix belongs to the instruction (decoding back from `c` finds `8d 05`,
        `lea eax, [rip+X]`, inside `48 8d 05`, one byte late); else, or when that decode does
        not land on an instruction holding `c`, the nearest start decoding back from `c` whose
        instruction holds `c` and names `target`."""
        f = self.func(c)
        if f:
            o = f[0]
            for a, size, _, _ in self.md.disasm_lite(self.d[o - self.base:c - self.base + 16], o):
                if a + size > c:
                    if a < c:
                        ins = next(self.md.disasm(self.d[a - self.base:a - self.base + 16], a), None)
                        if ins and self.operand_addr(ins) == target:
                            return a
                    break
        for back in range(1, 12):
            o = c - back
            ins = next(self.md.disasm(self.d[o - self.base:o - self.base + 16], o), None)
            if ins and ins.address + ins.size > c and self.operand_addr(ins) == target:
                return ins.address
        return None


RIP_REL = re.compile(r"rip ([+-]) (0x[0-9a-f]+)\]")
HEX = re.compile(r"0x[0-9a-f]{6,8}\b")


def _np():
    import numpy
    return numpy


# --- whole-section scans (numpy), shared with the IW tools' own image ---------------------------
def code_sections(sections):
    """(rva, size) of the executable sections of a `sections` list (name, rva, size, flags)."""
    return [(sva, vsz) for _, sva, vsz, ch in sections if ch & SCN_CODE]


def section_bytes(mem, sva, count, dtype=None):
    """`count` bytes of `mem` from offset `sva`, widened to int64 (or `dtype`) for arithmetic."""
    np = _np()
    return np.frombuffer(mem, dtype=np.uint8, count=count, offset=sva).astype(dtype or np.int64)


def le_s32(b, start, n):
    """The signed little-endian dwords at b[start + i], i < n (b: section_bytes)."""
    np = _np()
    d = (b[start:start + n] | (b[start + 1:start + n + 1] << 8)
         | (b[start + 2:start + n + 2] << 16) | (b[start + 3:start + n + 3] << 24))
    return np.where(d >= 1 << 31, d - (1 << 32), d)


def rel32_branches(mem, base, sections, tail=4):
    """For each code section: (rva, size, opcode bytes, targets): targets[i] is where an E8/E9
    rel32 opening at offset i would land (the caller filters on the opcode). `tail`: how many
    final offsets of the section are left out (4: every rel32 that fits; the IW scans use 5)."""
    np = _np()
    for sva, vsz in code_sections(sections):
        b = section_bytes(mem, sva, vsz)
        n = len(b) - tail
        t = base + sva + np.arange(n, dtype=np.int64) + 5 + le_s32(b, 1, n)
        yield sva, vsz, b[:n], t
