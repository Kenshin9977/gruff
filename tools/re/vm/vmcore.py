"""The game-independent part of the offline script-VM harnesses: a flat x64 dump mapped under
Unicorn, its imports pointed at logged stubs, the module TLS block, a bump heap, a Python printf,
function replacement (`hook`) and observation (`watch`) by RVA, calls that raise `Fault` on an
emulator fault, a hook error or a stop request, and a backtrace of return addresses.

Each game module (t89vm.py for T8/T9) subclasses `GameVM` with its own addresses; t7vm.py keeps
its own copy of these helpers (it predates this module) and can move onto it later.

Research tool only: it imports Unicorn (GPL-2.0-only) from Python, like the rest of tools/re, and
writes data files (traces, facts); nothing here is linked into the Rust crates.
"""
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn, X  # noqa: E402

PRINTF = re.compile(r"%([-+ #0]*)(\*|\d+)?(\.\*|\.\d+)?(hh|h|ll|l|I64|I32|I|z|j|t|L)?([diouxXeEfgGcspn%])")
BIGHEAP = 0x7B0000000000
BIGHEAP_SZ = 0x20000000   # 512 MiB
STUBS = 0x7A0000000000    # one 16-byte slot per import


class Fault(Exception):
    pass


class GameVM:
    def __init__(self, exe, log=print, verbose=False):
        self.log = log
        self.verbose = verbose
        self.img = img = Image(exe)
        self.B = img.base
        self.emu = emu = Emu(img, null_page=True)
        self.uc = uc = emu.uc
        uc.mem_map(BIGHEAP, BIGHEAP_SZ)
        self.big_next = BIGHEAP
        self.stubs = {}          # name -> {"why":..., "hits": n}
        self.unmapped = []       # (rva of the instruction, address) of each zero page mapped
        self.boot_log = []
        self.hook_error = self.stop_reason = None
        self.prints = []
        self._imports()
        uc.hook_add(unicorn.UC_HOOK_MEM_UNMAPPED, self.guarded(self._on_unmapped))
        self._tls()

    # --- memory ----------------------------------------------------------------------------
    def r8(self, va):
        return self.uc.mem_read(va, 1)[0]

    def r32(self, va):
        return self.emu.r32(va)

    def r64(self, va):
        return self.emu.r64(va)

    def w8(self, va, v):
        self.uc.mem_write(va, bytes([v & 0xFF]))

    def w32(self, va, v):
        self.emu.w32(va, v & 0xFFFFFFFF)

    def w64(self, va, v):
        self.uc.mem_write(va, struct.pack("<Q", v & (1 << 64) - 1))

    def va(self, rva):
        return self.B + rva

    def big_alloc(self, n, align=16):
        a = (self.big_next + align - 1) & ~(align - 1)
        self.big_next = a + n
        if self.big_next > BIGHEAP + BIGHEAP_SZ:
            raise Fault("big heap exhausted")
        return a

    def put_cstr(self, s):
        b = s.encode("latin-1") + b"\0"
        a = self.big_alloc(len(b), 16)
        self.uc.mem_write(a, b)
        return a

    def cstr(self, va, n=256):
        if not va:
            return None
        out = b""
        while len(out) < n:
            k = min(0x100 - (va & 0xFF), n - len(out))  # never read across a page boundary
            try:
                b = bytes(self.uc.mem_read(va, k))
            except unicorn.UcError:
                break
            z = b.find(b"\0")
            if z >= 0:
                return (out + b[:z]).decode("latin-1")
            out += b
            va += k
        return out.decode("latin-1")

    def _tls(self):
        """The module's thread-local block, copied from the TLS template, at
        ThreadLocalStoragePointer[_tls_index]."""
        d = self.img.d
        e = struct.unpack_from("<I", d, 0x3C)[0]
        trva, tsz = struct.unpack_from("<II", d, e + 24 + 112 + 8 * 9)
        self.tls_block = None
        if not tsz:
            return
        start, end, idx_va, _, zero = struct.unpack_from("<QQQQI", d, trva)
        tmpl = bytes(self.uc.mem_read(start, end - start)) if end > start else b""
        block = self.big_alloc(len(tmpl) + zero + 0x1000, 64)
        self.uc.mem_write(block, tmpl)
        index = self.r32(idx_va)
        arr = self.r64(self.emu.teb + 0x58)
        self.w64(arr + 8 * index, block)
        self.tls_block = block

    # --- stubs -----------------------------------------------------------------------------
    def note(self, name, why):
        s = self.stubs.setdefault(name, {"why": why, "hits": 0})
        s["hits"] += 1

    def _heap_alloc(self, n, flags=0):
        a = self.big_alloc(max(n, 1), 16)
        self.heap_sizes[a] = n
        return a

    def _heap_realloc(self, old, n):
        a = self._heap_alloc(n)
        if old:
            k = min(n, self.heap_sizes.get(old, 0))
            self.uc.mem_write(a, bytes(self.uc.mem_read(old, k)))
        return a

    def _slot_alloc(self):
        k = len(self.slots) + 1
        self.slots[k] = 0
        return k

    def _imports(self):
        """Point every import slot at a stub of ours: each returns 0 unless a handler says
        otherwise, and is logged by name when called."""
        d = self.img.d
        e = struct.unpack_from("<I", d, 0x3C)[0]
        irva = struct.unpack_from("<I", d, e + 24 + 112 + 8)[0]
        self.import_names = {}
        uc = self.uc
        uc.mem_map(STUBS, 0x100000)
        k = 0
        o = irva
        while irva:
            oft, _, _, name, ft = struct.unpack_from("<IIIII", d, o)
            if not name:
                break
            dll = self.img.cstr(self.B + name) or "?"
            src = oft or ft
            i = 0
            while True:
                ent = struct.unpack_from("<Q", d, src + 8 * i)[0]
                if not ent:
                    break
                if ent >> 63:
                    fn = "%s!#%d" % (dll, ent & 0xFFFF)
                elif self.img.in_image(self.B + (ent & 0x7FFFFFFF) + 2):
                    fn = "%s!%s" % (dll, self.img.cstr(self.B + (ent & 0x7FFFFFFF) + 2))
                else:
                    fn = "%s!?%#x" % (dll, ent)
                slot = STUBS + 16 * k
                uc.mem_write(slot, b"\xc3")
                self.w64(self.B + ft + 8 * i, slot)
                self.import_names[slot] = fn
                k += 1
                i += 1
            o += 20
        self.import_handlers = {
            "HeapAlloc": lambda vm: vm._heap_alloc(vm.emu.arg(2), vm.emu.arg(1)),
            "HeapReAlloc": lambda vm: vm._heap_realloc(vm.emu.arg(2), vm.emu.arg(3)),
            "HeapFree": lambda vm: 1,
            "HeapSize": lambda vm: vm.heap_sizes.get(vm.emu.arg(2), 0),
            "GetProcessHeap": lambda vm: 0x10000,
            "FlsAlloc": lambda vm: vm._slot_alloc(),
            "TlsAlloc": lambda vm: vm._slot_alloc(),
            "FlsGetValue": lambda vm: vm.slots.get(vm.emu.arg(0) & 0xFFFFFFFF, 0),
            "TlsGetValue": lambda vm: vm.slots.get(vm.emu.arg(0) & 0xFFFFFFFF, 0),
            "FlsSetValue": lambda vm: vm.slots.__setitem__(vm.emu.arg(0) & 0xFFFFFFFF, vm.emu.arg(1)) or 1,
            "TlsSetValue": lambda vm: vm.slots.__setitem__(vm.emu.arg(0) & 0xFFFFFFFF, vm.emu.arg(1)) or 1,
            "EncodePointer": lambda vm: vm.emu.arg(0),
            "DecodePointer": lambda vm: vm.emu.arg(0),
        }
        self.slots = {}
        self.heap_sizes = {}
        if k:
            uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(self._on_import), begin=STUBS,
                        end=STUBS + 16 * k)

    def _on_import(self, uc, addr, size, ud):
        fn = self.import_names.get(addr, "?")
        short = fn.split("!", 1)[1]
        h = self.import_handlers.get(short)
        if h:
            rax = h(self)
            self.note("import " + fn, "handled")
        else:
            rax = 0
            self.note("import " + fn, "returns 0")
        uc.reg_write(X.UC_X86_REG_RAX, rax or 0)

    # --- printf ----------------------------------------------------------------------------
    def format(self, fmt, arg):
        """C printf; arg(k) gives the k-th 8-byte argument slot."""
        out = []
        i = k = 0
        while i < len(fmt):
            c = fmt[i]
            m = PRINTF.match(fmt, i) if c == "%" else None
            if not m:
                out.append(c)
                i += 1
                continue
            i = m.end()
            flags, width, prec, length, conv = m.groups()
            if conv == "%":
                out.append("%")
                continue
            if width == "*":
                width = str(struct.unpack("<i", struct.pack("<I", arg(k) & 0xFFFFFFFF))[0])
                k += 1
            if prec == ".*":
                prec = "." + str(arg(k) & 0x7FFFFFFF)
                k += 1
            v = arg(k)
            k += 1
            spec = "%" + (flags or "") + (width or "") + (prec or "")
            bits = 64 if length in ("ll", "I64", "I", "z", "j", "t") else 32
            if conv in "di":
                v &= (1 << bits) - 1
                if v >> (bits - 1):
                    v -= 1 << bits
                out.append((spec + "d") % v)
            elif conv in "uxXo":
                out.append((spec + conv) % (v & ((1 << bits) - 1)))
            elif conv == "p":
                out.append("%016X" % v)
            elif conv == "c":
                out.append(chr(v & 0xFF))
            elif conv in "fgeEG":
                out.append((spec + conv) % struct.unpack("<d", struct.pack("<Q", v))[0])
            elif conv == "s":
                out.append((spec + "s") % (self.cstr(v, 4096) if v else "(null)"))
            else:
                out.append(m.group(0))
        return "".join(out)

    def vsnprintf_hook(self):
        """CRT vsnprintf(buf, size, fmt, va_list) in Python."""
        buf, size, fmt, va = (self.emu.arg(i) for i in range(4))
        text = self.format(self.cstr(fmt, 4096) or "", lambda k: self.r64(va + 8 * k))
        b = text.encode("latin-1", "replace")
        if size:
            n = min(len(b), size - 1)
            self.uc.mem_write(buf, b[:n] + bytes(1))
        return len(b) if len(b) < size else -1

    def snprintf_hook(self):
        """CRT snprintf(buf, size, fmt, ...) in Python."""
        buf, size, fmt = self.emu.arg(0), self.emu.arg(1), self.emu.arg(2)
        text = self.format(self.cstr(fmt, 4096) or "", lambda k: self.emu.arg(3 + k))
        b = text.encode("latin-1", "replace")
        if size:
            n = min(len(b), size - 1)
            self.uc.mem_write(buf, b[:n] + bytes(1))
        return len(b)

    # --- hooks -----------------------------------------------------------------------------
    def guarded(self, fn):
        """A hook callback whose Python errors stop the emulation (an exception raised through
        Unicorn's C callback would crash the process); call() re-raises it."""
        def cb(uc, *a):
            try:
                return fn(uc, *a)
            except Exception as ex:  # noqa: BLE001 - reported by call()
                import traceback
                self.hook_error = "%s: %s\n%s" % (type(ex).__name__, ex, traceback.format_exc())
                uc.emu_stop()
                return False
        return cb

    def hook(self, rva, fn, name=None, why=""):
        """Replace the function at rva: fn(vm) returns rax (the function is not run); a return
        of False lets the function run after all."""
        va = self.B + rva
        label = name or "%#x" % rva

        def cb(uc, addr, size, ud):
            r = fn(self)
            if r is not False:
                self.note(label, why)
                self.emu.ret(r if r is not None else 0)
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(cb), begin=va, end=va)

    def hook_longjmp(self, rva, name="longjmp"):
        """Replace the CRT longjmp(jmp_buf, value) at rva by a direct restore of the MSVC x64
        _JUMP_BUFFER (rbx, rsp, rbp, rsi, rdi, r12-r15, rip, mxcsr, xmm6-15): the CRT version
        unwinds through RtlUnwindEx (an import stub here) after a stack-limit check on the TEB."""
        va = self.B + rva
        regs = ((0x08, X.UC_X86_REG_RBX), (0x10, X.UC_X86_REG_RSP), (0x18, X.UC_X86_REG_RBP),
                (0x20, X.UC_X86_REG_RSI), (0x28, X.UC_X86_REG_RDI), (0x30, X.UC_X86_REG_R12),
                (0x38, X.UC_X86_REG_R13), (0x40, X.UC_X86_REG_R14), (0x48, X.UC_X86_REG_R15),
                (0x50, X.UC_X86_REG_RIP))
        xmms = [getattr(X, "UC_X86_REG_XMM%d" % i) for i in range(6, 16)]

        def cb(uc, addr, size, ud):
            buf, value = self.emu.arg(0), self.emu.arg(1) & 0xFFFFFFFF
            for off, r in regs:
                uc.reg_write(r, self.r64(buf + off))
            for i, r in enumerate(xmms):
                lo, hi = struct.unpack("<QQ", bytes(uc.mem_read(buf + 0x60 + 0x10 * i, 16)))
                uc.reg_write(r, lo | hi << 64)
            uc.reg_write(X.UC_X86_REG_RAX, value or 1)
            self.note(name, "direct _JUMP_BUFFER restore (no SEH unwind)")
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(cb), begin=va, end=va)

    def watch(self, rva, fn):
        """Call fn(vm) when the instruction at rva runs (the code still runs)."""
        va = self.B + rva
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(lambda uc, a, s, u: fn(self)),
                         begin=va, end=va)

    def _on_unmapped(self, uc, access, addr, size, value, ud):
        ip = uc.reg_read(X.UC_X86_REG_RIP)
        if access == unicorn.UC_MEM_FETCH_UNMAPPED:
            return False
        if len(self.unmapped) > 512:
            return False
        try:
            uc.mem_map(addr & ~0xFFF, 0x1000)
        except unicorn.UcError:
            return False
        self.unmapped.append((ip - self.B, addr))
        return True

    # --- calls -----------------------------------------------------------------------------
    def call(self, rva, *args, count=50_000_000):
        """Run the function at rva (or a full VA) to its return. A fault, a hook error or a
        stop request (self.stop_reason, set by a hook) raises Fault."""
        self.hook_error = None
        self.stop_reason = None
        va = rva if rva > self.B else self.B + rva
        try:
            r = self.emu.call(va, args, count=count)
        except unicorn.UcError as ex:
            raise Fault("%s at rva %#x (called %#x)\n%s" % (ex, self.emu.ip() - self.B, va - self.B,
                                                            self.backtrace()))
        if self.hook_error:
            raise Fault("hook error at rva %#x: %s" % (self.emu.ip() - self.B, self.hook_error))
        if self.stop_reason:
            raise Fault(self.stop_reason)
        if self.emu.ip() != self.emu.retaddr:
            raise Fault("instruction limit (%d) reached at rva %#x\n%s"
                        % (count, self.emu.ip() - self.B, self.backtrace()))
        return r

    def backtrace(self, n=24):
        """Return addresses on the stack that land after a call instruction in the image."""
        sp = self.uc.reg_read(X.UC_X86_REG_RSP)
        out = []
        for k in range(0, 0x4000, 8):
            try:
                v = self.r64(sp + k)
            except unicorn.UcError:
                break
            if self.img.in_image(v) and self.img.is_code(v):
                b = self.img.d[v - self.B - 5]
                b6 = self.img.d[v - self.B - 6:v - self.B - 4]
                if b == 0xE8 or b6 == b"\xff\x15":
                    out.append("  sp+%#x -> %#x" % (k, v - self.B))
            if len(out) >= n:
                break
        return "\n".join(out)
