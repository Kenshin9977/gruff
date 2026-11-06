"""Engine-emitted events (the notifies scripts `waittill`): the pieces shared by every family.

What an engine supplies (its profile, in treyarch/events*.py or iw/events*.py):

- the string constants the engine notifies with, when it uses any (`scr_const` slots filled at
  init by SL_GetString): slot -> string by emulating the init with the allocator hooked
  (`const_slots`);
- the core notify of the VM (`Notifier`) and how to read its arguments (inst, object id, event,
  count). The wrappers around it (entity / level, by id, by C string, by SL string) are found
  from the core by emulating each caller with sentinel arguments (`discover_wrappers`): the
  argument that reaches the core's event parameter is the wrapper's, nothing is taken from a
  name or a signature;
- the VM stack (`vmpub top` address per inst, value size, type offset) and the type-name table,
  so that each Scr_Add* push can be recognised by running it (`Pushers`): the type tag it writes
  at the new stack top is its type.

Each call site of a notifier is measured by a backward-window emulation (`SiteResolver`): from
up to k instructions before the call, never further back than the previous notify call, to the
notifier's entry reached from that site. Every call inside the window is skipped and recorded
(pushers give the argument types); scr_const slots hold tokens. The event argument at the entry
is a token (a scr_const slot: named), a C string in the image (named), a 32/64-bit immediate (a
hash: named when a candidate string hashes to it) or unresolved; unresolved sites are reported,
never dropped. The argument count is the count argument when it is a small integer (two
measures: it must equal the pushes seen in the window), else the pushes seen.
"""
import bisect
import re
import collections
import struct

from .image import _np, user_site
from .emu import Emu, unicorn, X

user_site()

M64 = (1 << 64) - 1
TOKEN_BASE = 0x7A000000      # 32-bit scr_const tokens
TOKEN16 = 0x4000             # 16-bit scr_const tokens
SENT = 0x5E000000            # sentinel argument i = SENT + i * 0x10000 (mappable, distinct)
PROBE_RET = 0x6D000000       # value a skipped call returns (its index * 0x100 added)
MAGIC = 0x3C000000           # global probe values (index added)
CODE = 0x20000000


def sentinel(i):
    """Argument i's probe value: distinct in its low 16 bits too (16-bit string ids)."""
    return SENT + i * 0x10000 + 0x5E00 + i


def _sent_match(v, off):
    if v is None:
        return None
    v &= 0xFFFFFFFFFFFFFFFF
    if v < 0x10000:  # truncated to a 16-bit id
        k = v - (0x5E00 + off)
        return k if 0 <= k < 16 else None
    v &= 0xFFFFFFFF
    k = (v - SENT) >> 16
    if 0 <= k < 16 and v == sentinel(k) + off:
        return k
    return None


def sentinel_index(v):
    return _sent_match(v, 0)


def sentinel_ptr_index(v):
    """Sentinel i's memory holds sentinel(i) + 0x80: a value read through argument i."""
    return _sent_match(v, 0x80)


# ---------------------------------------------------------------------------------------------
class CallIndex:
    """Every E8 (call) / E9 (jmp) rel32 in the code sections, indexed by target (one scan)."""

    def __init__(self, img):
        np = _np()
        self.img = img
        sites, tgts, kinds = [], [], []
        for name, sva, vsz, ch in img.sections:
            if not ch & CODE:
                continue
            b = np.frombuffer(img.d, dtype=np.uint8, count=vsz, offset=sva)
            n = len(b) - 4
            op = b[:n]
            idx = np.nonzero((op == 0xE8) | (op == 0xE9))[0]
            bb = b.astype(np.int64)
            disp = bb[idx + 1] | (bb[idx + 2] << 8) | (bb[idx + 3] << 16) | (bb[idx + 4] << 24)
            disp = np.where(disp >= 1 << 31, disp - (1 << 32), disp)
            t = img.base + sva + idx + 5 + disp
            keep = (t >= img.base) & (t < img.base + img.size)
            sites.append(img.base + sva + idx[keep])
            tgts.append(t[keep])
            kinds.append(op[idx[keep]] == 0xE8)
        s = np.concatenate(sites)
        t = np.concatenate(tgts)
        k = np.concatenate(kinds)
        o = np.argsort(t, kind="stable")
        self.t, self.s, self.k = t[o], s[o], k[o]

    def callers(self, target):
        """[(site va, "call"|"jmp")] of the rel32 bytes landing on `target` (not decoded: the
        Decoder confirms a site is an instruction of its function)."""
        np = _np()
        lo = int(np.searchsorted(self.t, target, "left"))
        hi = int(np.searchsorted(self.t, target, "right"))
        return [(int(self.s[i]), "call" if self.k[i] else "jmp") for i in range(lo, hi)]


class Decoder:
    """Linear decoding of whole functions (cached): an instruction is decoded from its function's
    start, never from a guessed boundary. x64: the .pdata chunk; x86 (no .pdata): the start is
    the nearest 16-aligned address after int3/nop padding from which a linear decode lands on
    the instruction."""

    def __init__(self, img, ci=None):
        self.img = img
        self.ci = ci
        self.cache = {}

    def _lands(self, a, va):
        img = self.img
        for ins in img.md.disasm(img.d[a - img.base:va + 16 - img.base], a):
            if ins.address == va:
                return True
            if ins.address > va:
                return False
        return False

    def start_of(self, va):
        img = self.img
        f = img.func(va)
        if f:
            return f[0], f[1]
        # no .pdata (x86, or an x64 leaf): the nearest rel32 call/jmp target at or before va
        # (within 0x200 bytes, not inside a .pdata function) from which a linear decode lands
        # on va; else the nearest 16-aligned address after int3/nop/ret padding.
        # (x86: within 0x20000 bytes, the 8 nearest targets)
        if self.ci is not None:
            np = _np()
            span = 0x200 if img.x64 else 0x20000
            hi = int(np.searchsorted(self.ci.t, va, "right"))
            lo = int(np.searchsorted(self.ci.t, va - span, "left"))
            for t in sorted({int(x) for x in self.ci.t[lo:hi]}, reverse=True)[:8]:
                if img.func(t):
                    break
                if self._lands(t, va):
                    return t, None
        a = va & ~0xF
        for _ in range(0x100 if not img.x64 else 0x40):
            if a <= img.base or (a != (va & ~0xF) and img.func(a)):
                break
            prev = img.d[a - img.base - 1]
            if prev in (0xCC, 0x90, 0xC3) and self._lands(a, va):
                return a, None
            a -= 0x10
        return None, None

    def _end(self, start, limit=0x10000):
        """End of a function without .pdata: the first ret, jmp (a tail call included) or int3
        past which no forward branch of the function reaches."""
        img = self.img
        reach = start
        last = start
        for ins in img.md.disasm(img.d[start - img.base:start - img.base + limit], start):
            last = ins.address + ins.size
            m = ins.mnemonic
            if m.startswith("j") and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                if start < t < start + limit and m != "jmp":
                    reach = max(reach, t)
                elif m == "jmp" and ins.address < t < ins.address + 0x800 and t > reach:
                    reach = t  # a near forward jmp stays in the function; a far one is a tail call
            if (m.startswith("ret") or m == "jmp" or m == "int3") and reach <= ins.address:
                return last
        return last

    def function_insns(self, va):
        img = self.img
        start, end = self.start_of(va)
        if start is None:
            return None, None, None
        cached = self.cache.get(start)
        if cached is None or (end is None and cached[1] and cached[1][-1] < va):
            if end is None:
                end = max(self._end(start), va + 16)
            ins = list(img.md.disasm(img.d[start - img.base:end - img.base], start))
            self.cache[start] = (ins, [i.address for i in ins])
            if len(self.cache) > 3000:
                self.cache.pop(next(iter(self.cache)))
        ins, addrs = self.cache[start]
        return start, ins, addrs

    def at(self, va):
        """(function start, instruction list, index of the instruction at va) or Nones."""
        start, ins, addrs = self.function_insns(va)
        if ins is None:
            return None, None, None
        i = bisect.bisect_left(addrs, va)
        if i < len(addrs) and addrs[i] == va:
            return start, ins, i
        return start, None, None


# ---------------------------------------------------------------------------------------------
def const_slots(img, init_funcs, alloc_fn, str_arg=0, width=4, log=None, count=5_000_000,
                out_arg=None, decrypt=None):
    """Emulate the init function(s) filling the string constants, with the string allocator
    (`alloc_fn`, the engine's SL_GetString) hooked: each call returns a token and records its
    string; a write of a token into the image gives slot -> string. `width` 4: 32-bit ids, 2:
    16-bit ids (IW3-IW5). `out_arg`: the allocator writes the id through that argument and
    returns the pointer (BO6: getter(&out, char*, 0x20, 1), then `mov ecx, [rax]`).
    `decrypt`: the engine's string decryption (dst, size, src, &flag) -> text, which the
    allocator runs on its argument (MWIII): run afterwards on each recorded string. Returns
    ({slot va: string}, {slot va: write size}, strings in call order, errors)."""
    emu = Emu(img)
    strs = []
    ptrs = []
    base = TOKEN_BASE if width == 4 else TOKEN16
    mask = 0xFFFFFFFF if width == 4 else 0xFFFF

    def on_alloc(uc, a, s, u):
        p = emu.arg(str_arg)
        ptrs.append(p)
        raw = bytes(uc.mem_read(p, 256)).split(b"\0")[0]
        strs.append(raw.decode("latin1"))
        if out_arg is None:
            emu.ret(base + len(strs) - 1)
        else:
            o = emu.arg(out_arg)
            emu.w32(o, base + len(strs) - 1)
            emu.ret(o)

    slots, sizes = {}, {}

    def on_write(uc, acc, a, size, v, u):
        if size >= width and img.in_image(a):
            k = (v & mask) - base
            if 0 <= k < len(strs):
                slots[a] = k
                sizes[a] = size

    h1 = emu.uc.hook_add(unicorn.UC_HOOK_CODE, on_alloc, begin=alloc_fn, end=alloc_fn)
    h2 = emu.uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, on_write)
    emu.map_on_unmapped(256)
    errs = []
    for f in init_funcs:
        emu.pages = 0
        r, err = emu.run(f, count=count)
        if err:
            errs.append("%#x: %s" % (f - img.base, err))
    if decrypt is not None:
        emu.uc.hook_del(h1)
        emu.uc.hook_del(h2)
        buf, flag = emu.alloc(0x2000), emu.alloc(16)
        bad = 0
        for k, p in enumerate(ptrs):
            r, err = emu.run(decrypt, [buf, 0x1000, p, flag], count=2_000_000)
            try:
                raw = bytes(emu.uc.mem_read(r, 256)).split(b"\0")[0] if r and not err else b""
            except unicorn.UcError:
                raw = b""
            if err or not raw or not all(32 <= c < 127 for c in raw):
                bad += 1
                strs[k] = ""  # not decrypted: never a name
            else:
                strs[k] = raw.decode("latin1")
        if log is not None:
            log.append("const strings decrypted by %#x: %d of %d (%d not)" % (
                decrypt - img.base, len(ptrs) - bad, len(ptrs), bad))
    slots = {a: strs[k] for a, k in slots.items() if strs[k]}
    if log is not None:
        log.append("const init: %d allocator calls, %d slots written, %d distinct strings; "
                   "errors %s" % (len(strs), len(slots), len(set(slots.values())), errs or "none"))
    return slots, sizes, strs, errs


def type_name_table(img, first=("undefined",), second=("pointer", "object")):
    """The VM's type-name table: a pointer array whose entries are "undefined", then "pointer"
    or "object", then "string"... Returns [names] (stops at the first non-string)."""
    cands = set()
    for s in first:
        cands |= set(img.find_all(s.encode() + b"\0"))
    refs = img.abs_refs(cands)
    best = []
    for t, rs in refs.items():
        for r in rs:
            if r % img.ptr:
                continue
            nxt = img.cstr(img.uptr(r + img.ptr))
            if nxt not in second:
                continue
            names = []
            k = 0
            while True:
                s = img.cstr(img.uptr(r + img.ptr * k)) if img.in_image(img.uptr(r + img.ptr * k)) else None
                if not s:
                    break
                names.append(s)
                k += 1
            if len(names) > len(best):
                best = names
    return best


# ---------------------------------------------------------------------------------------------
class Notifier:
    """A notify entry point. Argument specs are ("arg", i) (the i-th argument), ("const", v),
    ("global", va) (read from image data) or None. event_kind: "id" (a string id or hash),
    "cstr" (a C string), "sl" (an SL string id converted to its text then hashed)."""

    def __init__(self, va, label, inst, obj, event, count, event_kind="id", recv=None,
                 parent=None, evidence=""):
        self.va, self.label = va, label
        self.inst, self.obj, self.event, self.count = inst, obj, event, count
        self.event_kind = event_kind
        self.recv = recv          # "entity" | "level" | None (decided by the object argument)
        self.parent = parent
        self.evidence = evidence

    def as_dict(self, base):
        return {"rva": self.va - base, "label": self.label, "inst": self.inst, "obj": self.obj,
                "event": self.event, "count": self.count, "event_kind": self.event_kind,
                "recv": self.recv, "parent": (self.parent - base) if self.parent else None,
                "evidence": self.evidence}


class Probe:
    """One Emu shared by the wrapper probes, the site windows and the pusher runs."""

    def __init__(self, img, dec, prof):
        self.img, self.dec, self.prof = img, dec, prof
        self.emu = Emu(img, null_page=True)
        # the ranges the probe values point into are mapped once (zeros): a run maps few pages
        # on demand (Unicorn grows fragile with thousands of small regions)
        for lo, sz in ((SENT, 0x200000), (PROBE_RET, 0x200000), (MAGIC, 0x100000),
                       (0x4F000000, 0x1000000)):
            self.emu.uc.mem_map(lo, sz)
        self.page_cap = 64
        self.emu.uc.hook_add(unicorn.UC_HOOK_MEM_READ_UNMAPPED | unicorn.UC_HOOK_MEM_WRITE_UNMAPPED,
                         self._on_unmapped)
        # never execute outside the image's code (a zero page run as code writes into itself),
        # except the `ret` pages standing for imports
        self.stubs = set()
        self.emu.uc.hook_add(unicorn.UC_HOOK_MEM_FETCH_UNMAPPED, self._on_fetch)
        self.emu.uc.hook_add(unicorn.UC_HOOK_BLOCK, self._on_block)
        # code is read+execute only: a stray write into it (garbage pointers in a probe) stops
        # the run instead of making Unicorn retranslate code (which crashes the host process)
        for name, sva, vsz, ch in img.sections:
            if ch & CODE:
                lo = (img.base + sva) & ~0xFFF
                hi = (img.base + sva + vsz + 0xFFF) & ~0xFFF
                self.emu.uc.mem_protect(lo, hi - lo, unicorn.UC_PROT_READ | unicorn.UC_PROT_EXEC)
        self.uc = self.emu.uc
        self.strbuf = self.emu.alloc(0x10000)
        self.skipped = []
        self.reads = []
        self.entry = {}
        self.window = None
        self.last = None
        self.target = None
        self.capture = None
        self.reached = False
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self._on_code)
        self.uc.hook_add(unicorn.UC_HOOK_MEM_READ, self._on_read)
        self.calls = {}  # call instruction va -> (size, target or None)
        self.watch_reads = False
        self.read_magic = {}
        self.read_saved = {}
        self.ret_strings = None
        self.guide = None
        self.keep_calls = None
        self.run_calls = False
        self.zero_returns = False

    # --- hooks ---------------------------------------------------------------------------------
    def _on_unmapped(self, uc, access, addr, size, value, ud):
        if self.emu.pages > self.page_cap:
            return False
        try:
            uc.mem_map(addr & ~0xFFF, 0x1000)
        except unicorn.UcError:
            return False
        self.emu.pages += 1
        return True

    def _on_fetch(self, uc, access, addr, size, value, ud):
        """A call through the import table into a DLL the dump does not hold: a page of `ret`
        (returns 0 in rax... whatever rax holds) stands for the import."""
        if len(self.stubs) > 256:
            return False
        page = addr & ~0xFFF
        try:
            uc.mem_map(page, 0x1000, unicorn.UC_PROT_READ | unicorn.UC_PROT_EXEC)
            uc.mem_write(page, b"\xc3" * 0x1000)
        except unicorn.UcError:
            return False
        self.stubs.add(page)
        return True

    def _on_block(self, uc, addr, size, ud):
        if not self.img.is_code(addr) and (addr & ~0xFFF) not in self.stubs:
            uc.emu_stop()

    def _on_code(self, uc, addr, size, ud):
        if addr == self.target and self.capture is not None:
            if self.window is None or self.last == self.site:
                self.reached = True
                self.capture()
                uc.emu_stop()
                return
        lo, hi = self.window if self.window else (None, None)
        if self.window is not None and not (lo <= addr <= hi):
            uc.emu_stop()
            return
        self.last = addr
        if self.guide:
            nxt = self.guide.get(addr)
            if nxt is not None:  # a branch only one side of which reaches the site: take it
                uc.reg_write(self.emu.ip_reg, nxt)
                return
        c = self.calls.get(addr)
        if c is None and self.keep_calls is not None:
            # pusher mode: every call is decoded here; calls to the push cores run, others skip
            b = bytes(uc.mem_read(addr, 2))
            if b[0] == 0xE8 or b[0] == 0xFF or (b[0] & 0xF0 == 0x40 and b[1] == 0xFF):
                ins = next(self.img.md.disasm(bytes(uc.mem_read(addr, 16)), addr), None)
                if ins is not None and ins.mnemonic == "call":
                    t = int(ins.op_str, 16) if ins.op_str.startswith("0x") else None
                    if t in self.keep_calls:
                        return
                    c = (ins.size, t)
        if c is not None:
            csize, tgt = c
            if tgt is not None and tgt == self.target and (self.window is None or addr == self.site):
                return
            if self.window is not None and addr == self.site:
                return
            if self.run_calls:  # string probe: the wrapper's own hashing callee must run
                return
            # skip the call: record it with its first arguments, return a probe value
            k = len(self.skipped)
            args = self._args_at_call()
            self.skipped.append((addr, tgt, args))
            uc.reg_write(self.emu.ip_reg, addr + csize)
            if self.ret_strings is not None:
                rv = self.put_str("evret%d" % k)
                self.ret_strings[k] = "evret%d" % k
            else:
                rv = 0 if self.zero_returns else PROBE_RET + k * 0x100
            uc.reg_write(self.emu.ax_reg, rv)

    def _args_at_call(self):
        """Arguments at a call instruction (before it executes: no return address yet)."""
        e = self.emu
        uc = self.uc
        if e.x64:
            return [uc.reg_read(r) for r in (X.UC_X86_REG_RCX, X.UC_X86_REG_RDX,
                                             X.UC_X86_REG_R8, X.UC_X86_REG_R9)]
        sp = uc.reg_read(X.UC_X86_REG_ESP)
        out = []
        for i in range(4):
            try:
                out.append(e.r32(sp + 4 * i))
            except unicorn.UcError:
                out.append(0)
        out += [uc.reg_read(X.UC_X86_REG_ECX), uc.reg_read(X.UC_X86_REG_EDX),
                uc.reg_read(X.UC_X86_REG_EAX)]
        return out

    def _on_read(self, uc, acc, addr, size, value, ud):
        if not self.watch_reads or size < 4 or not self.img.in_image(addr):
            return
        if self.img.is_code(addr):
            return
        if addr not in self.read_magic:
            self.read_magic[addr] = MAGIC + len(self.read_magic) * 0x10
            self.read_saved[addr] = bytes(uc.mem_read(addr, 4))  # what the emulator held
        uc.mem_write(addr, struct.pack("<I", self.read_magic[addr]))

    # --- helpers -------------------------------------------------------------------------------
    def index_calls(self, start, insns):
        for ins in insns:
            if ins.mnemonic == "call":
                t = int(ins.op_str, 16) if ins.op_str.startswith("0x") else None
                self.calls[ins.address] = (ins.size, t)

    def put_str(self, s):
        a = self.strbuf
        self.strbuf += (len(s) + 16) & ~15
        self.uc.mem_write(a, s.encode("latin1") + b"\0")
        return a

    def read_spec(self, spec):
        if spec is None:
            return None
        kind, v = spec
        if kind == "arg":
            return self.emu.arg(v)
        if kind in ("const", "unknown"):  # unknown: the value the wrapper's probe saw, not
            return v                      # from its arguments (T11: a VM context's address)
        if kind == "gptr":  # a pointer read from image data, plus an offset (T11: the VM
            va, off = v     # context, [gScrVm[i]] + 0x10)
            return (self.emu.r64(va) + off) & M64
        if kind == "global":
            return self.emu.r32(v)
        if kind == "reg":  # a register argument (x86 usercall: IW3's paramcount in eax)
            return self.uc.reg_read(getattr(X, "UC_X86_REG_" + v.upper()))
        if kind == "deref":  # the argument points to the value (a 64-bit hash)
            try:
                return self.emu.r64(self.emu.arg(v))
            except unicorn.UcError:
                return None
        raise ValueError(spec)

    def reset_regs(self):
        uc = self.uc
        for i in range(16):  # sentinel i points to sentinel(i) + 0x80 (sentinel_ptr_index)
            uc.mem_write(sentinel(i), (sentinel(i) + 0x80).to_bytes(8, "little"))
        regs = ([X.UC_X86_REG_RAX, X.UC_X86_REG_RBX, X.UC_X86_REG_RCX, X.UC_X86_REG_RDX,
                 X.UC_X86_REG_RSI, X.UC_X86_REG_RDI, X.UC_X86_REG_RBP, X.UC_X86_REG_R8,
                 X.UC_X86_REG_R9, X.UC_X86_REG_R10, X.UC_X86_REG_R11, X.UC_X86_REG_R12,
                 X.UC_X86_REG_R13, X.UC_X86_REG_R14, X.UC_X86_REG_R15] if self.emu.x64 else
                [X.UC_X86_REG_EAX, X.UC_X86_REG_EBX, X.UC_X86_REG_ECX, X.UC_X86_REG_EDX,
                 X.UC_X86_REG_ESI, X.UC_X86_REG_EDI])
        for k, r in enumerate(regs):
            uc.reg_write(r, 0x4F000000 + k * 0x100000)
        if self.emu.x64:
            uc.reg_write(X.UC_X86_REG_RBP, self.emu.stack + 0x100000)
        else:
            uc.reg_write(X.UC_X86_REG_EBP, self.emu.stack + 0x100000)

    # --- 1. wrapper probe ---------------------------------------------------------------------
    def probe_wrapper(self, fstart, finsns, parent, nargs=6, string_args=False):
        """Run the function at fstart with sentinel arguments (or, with string_args, a probe C
        string per argument) until it enters `parent`; returns the parent's (inst, obj, event,
        count) values, the skipped calls and the global reads seen (magic values), or None."""
        self.calls = {}
        self.index_calls(fstart, finsns)
        self.skipped = []
        self.window = None
        self.target = parent.va
        self.site = None
        got = {}
        strings = {}

        def cap():
            got["inst"] = self.read_spec(parent.inst)
            got["obj"] = self.read_spec(parent.obj)
            got["event"] = self.read_spec(parent.event)
            got["count"] = self.read_spec(parent.count)
            if parent.event_kind == "cstr" and got["event"] is not None:
                try:
                    got["event_str"] = bytes(self.uc.mem_read(got["event"], 64)).split(b"\0")[0]
                except unicorn.UcError:
                    got["event_str"] = None
        self.capture = cap
        args = []
        for i in range(nargs):
            if string_args:
                s = "evprobe%d" % i
                a = self.put_str(s)
                strings[a] = s
                args.append(a)
            else:
                args.append(sentinel(i))
        saved = {}
        self.read_magic = {}
        self.watch_reads = True
        self.reset_regs()
        self.emu.pages = 0
        r, err = self.emu.run(fstart, args, count=200_000)
        self.watch_reads = False
        # restore the globals the probe overwrote
        for a, b in self.read_saved.items():  # (scr_const tokens included)
            self.uc.mem_write(a, b)
        self.read_saved = {}
        self.capture = None
        if not got:
            return None
        got["skipped"] = list(self.skipped)
        got["magic"] = {v: a for a, v in self.read_magic.items()}
        got["strings"] = strings
        got["err"] = err
        return got

    # --- 2. site window ------------------------------------------------------------------------
    def run_window(self, start, site, notifier, count=20_000, window=None, guide=None):
        """Emulate from `start` to the notifier's entry reached from `site` (instructions kept
        within `window`, default [start, site]; `guide` {branch va: forced next va}). Returns
        (values, skipped calls) or None when the entry is not reached that way."""
        self.skipped = []
        self.window = window or (start, site)
        self.guide = guide
        self.site = site
        self.target = notifier.va
        self.last = None
        got = {}

        def cap():
            got["inst"] = self.read_spec(notifier.inst)
            got["obj"] = self.read_spec(notifier.obj)
            got["event"] = self.read_spec(notifier.event)
            got["count"] = self.read_spec(notifier.count)
            if notifier.event_kind == "cstr" and got["event"]:
                try:
                    raw = bytes(self.uc.mem_read(got["event"], 128)).split(b"\0")[0]
                    if raw and all(32 <= c < 127 for c in raw):
                        got["event_str"] = raw.decode("latin1")
                except unicorn.UcError:
                    pass
        self.capture = cap
        self.reset_regs()
        sp = self.emu.stack + 0x1F0000 - 0x800
        self.uc.reg_write(self.emu.sp_reg, sp)
        self.emu.wptr(sp, self.emu.retaddr)
        for i in range(10):  # argument i at [sp + ptr * (1 + i)] (x64: home space, then stack)
            self.emu.wptr(sp + self.img.ptr * (1 + i), sentinel(i))
        if self.emu.x64:
            for i, r in enumerate((X.UC_X86_REG_RCX, X.UC_X86_REG_RDX, X.UC_X86_REG_R8,
                                   X.UC_X86_REG_R9)):
                self.uc.reg_write(r, sentinel(i))
        self.emu.pages = 0
        try:
            self.uc.emu_start(start, 0, count=count)
        except unicorn.UcError:
            pass
        self.capture = None
        self.window = None
        self.guide = None
        if not got:
            return None
        return got, list(self.skipped)


# ---------------------------------------------------------------------------------------------
def discover_wrappers(img, ci, dec, probe, core, hash_fn, log, max_size=0x300, max_depth=5):
    """From the core notifier, every caller function that forwards one of its own arguments to
    the event parameter (a sentinel, a C string hashed, an SL string converted then hashed) is a
    wrapper; recursively. Returns [Notifier] (core first) and the set of wrapper functions."""
    out = [core]
    queue = [(core, 0)]
    seen = {core.va}
    entity_resolvers = collections.Counter()
    while queue:
        n, depth = queue.pop(0)
        if depth >= max_depth:
            continue
        for site, kind in ci.callers(n.va):
            fstart, insns, idx = dec.at(site)
            if insns is None or fstart in seen:
                continue
            fend = insns[-1].address + insns[-1].size
            if fend - fstart > max_size:
                continue
            w = classify_wrapper(img, probe, fstart, insns, n, hash_fn, entity_resolvers)
            if w is None:
                continue
            seen.add(fstart)
            out.append(w)
            queue.append((w, depth + 1))
            log.append("wrapper %#x <- %#x: %s" % (fstart - img.base, n.va - img.base, w.evidence))
    return out, entity_resolvers


def classify_wrapper(img, probe, fstart, insns, parent, hash_fn, entity_resolvers):
    g = probe.probe_wrapper(fstart, insns, parent)
    if g is None:
        return None
    ev = g["event"]
    ev_kind = None
    ev_spec = None
    if parent.event_kind == "cstr":
        # parent takes a C string: the wrapper's argument is that pointer, or a string id
        si = sentinel_index(ev or 0)
        if si is not None:
            ev_spec, ev_kind = ("arg", si), "cstr"
    else:
        si = sentinel_index(ev or 0)
        pi = sentinel_ptr_index(ev)
        if si is not None:
            ev_spec, ev_kind = ("arg", si), parent.event_kind
        elif pi is not None:
            ev_spec, ev_kind = ("deref", pi), parent.event_kind
    if ev_spec is None and parent.event_kind != "cstr" and hash_fn is not None:
        # a C string argument hashed inside the wrapper
        g2 = probe.probe_wrapper(fstart, insns, parent, string_args=True)
        if g2 is None or g2["event"] is None or not any(
                (g2["event"] & M64) == hash_fn(x) for x in g2["strings"].values()):
            probe.run_calls = True  # the hash may be computed by a callee
            try:
                g2 = probe.probe_wrapper(fstart, insns, parent, string_args=True)
            finally:
                probe.run_calls = False
        if g2 is not None:
            for a, s in g2["strings"].items():
                if g2["event"] is not None and (g2["event"] & 0xFFFFFFFFFFFFFFFF) == hash_fn(s):
                    ev_spec, ev_kind = ("arg", int(s[7:])), "cstr"
                    break
        if ev_spec is None:
            # an SL id converted to a string by a skipped call, then hashed
            g3 = probe_sl(img, probe, fstart, insns, parent, hash_fn)
            if g3 is not None:
                ev_spec, ev_kind = g3[0], g3[1]  # receiver and count from the plain run
    if ev_spec is None:
        return None
    count = _spec_of(g["count"], g)
    inst = _spec_of(g["inst"], g)
    obj_v = g["obj"]
    recv = parent.recv
    obj = None
    if parent.obj is not None and parent.recv is None:
        si = sentinel_index(obj_v or 0)
        if si is not None:
            obj = ("arg", si)
        elif obj_v in g["magic"]:
            recv = "level"
        elif obj_v is not None and PROBE_RET <= (obj_v & 0xFFFFFFFF) < PROBE_RET + 0x100000:
            k = ((obj_v & 0xFFFFFFFF) - PROBE_RET) // 0x100
            recv = "entity"
            if k < len(g["skipped"]) and g["skipped"][k][1]:
                entity_resolvers[g["skipped"][k][1]] += 1
        else:
            recv = None
    elif parent.recv is None:
        recv = None
    ev_txt = "event = argument %d (%s)" % (ev_spec[1], ev_kind)
    return Notifier(fstart, "wrapper", inst, obj, ev_spec, count, ev_kind, recv, parent.va,
                    "%s; count %s; inst %s; receiver %s" % (ev_txt, count, inst, recv or
                                                             "from argument %s" % (obj,)))


def probe_sl(img, probe, fstart, insns, parent, hash_fn):
    """Wrapper that converts an SL string id (an argument) to its text through a call, then
    hashes it: every skipped call returns a probe string "evret<k>"; the event must be the hash
    of the string returned by a call that received a sentinel argument."""
    probe.ret_strings = {}
    try:
        g = probe.probe_wrapper(fstart, insns, parent)
        rets = dict(probe.ret_strings)
    finally:
        probe.ret_strings = None
    if g is None or g["event"] is None:
        return None
    for k, st in rets.items():
        if (g["event"] & 0xFFFFFFFFFFFFFFFF) == hash_fn(st):
            for v in g["skipped"][k][2]:
                si = sentinel_index(v)
                if si is not None:
                    return ("arg", si), "sl", g
    return None


def _spec_of(v, g):
    if v is None:
        return None
    si = sentinel_index(v)
    if si is not None:
        return ("arg", si)
    if v in g["magic"]:
        return ("global", g["magic"][v])
    for off in (0, 0x8, 0x10, 0x18, 0x20):  # a 64-bit global read (its low half the probe
        lo = (v - off) & 0xFFFFFFFF          # value) plus a small offset
        if v > 0xFFFFFFFF and lo in g["magic"]:
            return ("gptr", (g["magic"][lo], off))
    if (v & 0xFFFFFFFF) < 64:
        return ("const", v & 0xFFFFFFFF)
    return ("unknown", v)


IDENT_STR = re.compile(rb"(?<=\x00)[A-Za-z0-9_ .\-]{2,80}(?=\x00)")


def image_names(img, wanted, hash_fn, mask=0xFFFFFFFF):
    """{hash: sorted names}: the NUL-delimited identifier-like strings of the image's data
    sections whose hash (hash_fn) is in `wanted`."""
    wanted = set(wanted)
    out = collections.defaultdict(set)
    for name, sva, vsz, ch in img.sections:
        if ch & CODE:
            continue
        for m in IDENT_STR.finditer(img.d, sva, sva + vsz):
            s = m.group().decode("latin1")
            h = hash_fn(s) & mask
            if h in wanted:
                out[h].add(s)
    return {h: sorted(v) for h, v in out.items()}
