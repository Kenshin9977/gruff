"""The game-independent part of the offline script-VM harnesses for 32-bit x86 executables (World
at War, Call of Duty 4): the image mapped under Unicorn at its own base, its imports pointed at
logged stdcall stubs, the module TLS block, a bump heap, a Python printf over 4-byte argument
slots, function replacement (`hook`) and observation (`watch`) by VA, the MSVC x86 longjmp
replaced by a direct `_JUMP_BUFFER` restore, calls with register arguments (the LTCG builds pass
many arguments in eax/ecx/edx/esi/edi), and a backtrace of return addresses.

The x64 counterpart is vmcore.py (T8/T9); this file only adds, it changes nothing there. Every
stub is logged per run (`stubs`), every zero page mapped on demand too (`unmapped`).

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
BIGHEAP = 0x30000000
BIGHEAP_SZ = 0x30000000   # 768 MiB, below the 2 GiB line
STUBS = 0x2F000000        # one 16-byte slot per import

# stdcall argument counts of the x86 imports (a stub must pop them); an import missing here
# stops the run when called, rather than unbalancing the stack silently
X86_ARGS = {
    "GetLastError": 0, "SetLastError": 1, "EnterCriticalSection": 1, "LeaveCriticalSection": 1,
    "TryEnterCriticalSection": 1, "InitializeCriticalSection": 1, "DeleteCriticalSection": 1,
    "InitializeCriticalSectionAndSpinCount": 2,
    "InterlockedIncrement": 1, "InterlockedDecrement": 1, "InterlockedExchange": 2,
    "InterlockedCompareExchange": 3, "InterlockedExchangeAdd": 2,
    "timeGetTime": 0, "GetTickCount": 0, "QueryPerformanceCounter": 1,
    "QueryPerformanceFrequency": 1, "GetCurrentThreadId": 0, "GetCurrentThread": 0,
    "GetCurrentProcess": 0, "GetCurrentProcessId": 0, "Sleep": 1, "SleepEx": 2,
    "TlsGetValue": 1, "TlsSetValue": 2, "TlsAlloc": 0, "TlsFree": 1, "WaitForSingleObject": 2,
    "SetEvent": 1, "ResetEvent": 1, "GetProcessHeap": 0, "HeapAlloc": 3, "HeapFree": 3,
    "HeapReAlloc": 4, "HeapSize": 3, "OutputDebugStringA": 1, "IsDebuggerPresent": 0,
    "GetModuleHandleA": 1, "GetSystemTimeAsFileTime": 1, "GetLocalTime": 1,
    "GetSystemTime": 1, "MultiByteToWideChar": 6, "WideCharToMultiByte": 8,
    "SwitchToThread": 0, "FlsGetValue": 1, "FlsSetValue": 2, "FlsAlloc": 1, "EncodePointer": 1,
    "DecodePointer": 1, "GetStringTypeA": 5, "GetStringTypeW": 4, "LCMapStringA": 6,
    "LCMapStringW": 6, "GetCPInfo": 2, "GetACP": 0, "GetOEMCP": 0, "IsValidCodePage": 1,
    "VirtualAlloc": 4, "VirtualFree": 3, "VirtualQuery": 3, "VirtualProtect": 4,
    "GetFileAttributesA": 1, "CloseHandle": 1, "ReleaseSemaphore": 3,
    "WaitForMultipleObjects": 4, "GetModuleFileNameA": 3, "GetModuleHandleW": 1,
    "GetFileAttributesW": 1, "OutputDebugStringW": 1, "SetUnhandledExceptionFilter": 1,
    "UnhandledExceptionFilter": 1, "TerminateProcess": 2, "ExitProcess": 1,
    "RaiseException": 4, "RtlUnwind": 4, "GetProcAddress": 2, "LoadLibraryA": 1,
    "FreeLibrary": 1, "DuplicateHandle": 7, "SetThreadIdealProcessor": 2,
    "SetThreadPriority": 2, "GetProcessAffinityMask": 3, "SetThreadAffinityMask": 2,
}
PTR_RESULT = {"GetProcessHeap", "GetModuleHandleA", "GetCurrentThread", "GetCurrentProcess"}


class Fault(Exception):
    pass


class GameVM32:
    def __init__(self, exe, log=print, verbose=False):
        self.log = log
        self.verbose = verbose
        self.img = img = Image(exe)
        if img.x64:
            raise SystemExit("%s is x64: use vmcore.GameVM" % exe)
        self.B = img.base
        self.emu = emu = Emu(img, null_page=True)
        self.uc = uc = emu.uc
        uc.mem_map(BIGHEAP, BIGHEAP_SZ)
        self.big_next = BIGHEAP
        self.stubs = {}          # name -> {"why":..., "hits": n}
        self.unmapped = []       # (va of the instruction, address) of each zero page mapped
        self.boot_log = []
        self.hook_error = self.stop_reason = None
        self.prints = []
        self.slots = {}
        self.heap_sizes = {}
        self.clock_ms = 0
        self.clock_frame_ms = 50
        self._imports()
        uc.hook_add(unicorn.UC_HOOK_MEM_UNMAPPED, self.guarded(self._on_unmapped))
        # the first MiB is mapped as zeros so that null-plus-offset reads continue (absent engine
        # state); a write there means engine state the harness did not set up: recorded
        self.null_writes = {}    # eip -> (first address, count)
        uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, self._on_null_write, begin=0, end=0xFFFFF)
        self._tls()

    # --- memory ----------------------------------------------------------------------------
    def r8(self, va):
        return self.uc.mem_read(va, 1)[0]

    def r16(self, va):
        return struct.unpack("<H", self.uc.mem_read(va, 2))[0]

    def r32(self, va):
        return self.emu.r32(va)

    def w8(self, va, v):
        self.uc.mem_write(va, bytes([v & 0xFF]))

    def w16(self, va, v):
        self.uc.mem_write(va, struct.pack("<H", v & 0xFFFF))

    def w32(self, va, v):
        self.emu.w32(va, v & 0xFFFFFFFF)

    def reg(self, name):
        return self.uc.reg_read(getattr(X, "UC_X86_REG_" + name.upper()))

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
        """The module's thread-local block (x86 IMAGE_TLS_DIRECTORY32), copied from its template,
        at ThreadLocalStoragePointer[_tls_index] (fs:[0x2c])."""
        d = self.img.d
        e = struct.unpack_from("<I", d, 0x3C)[0]
        trva, tsz = struct.unpack_from("<II", d, e + 24 + 96 + 8 * 9)
        self.tls_block = None
        if not tsz:
            return
        start, end, idx_va, _, zero = struct.unpack_from("<IIIII", d, trva)
        tmpl = bytes(self.uc.mem_read(start, end - start)) if end > start else b""
        block = self.big_alloc(len(tmpl) + zero + 0x1000, 64)
        self.uc.mem_write(block, tmpl)
        index = self.r32(idx_va)
        arr = self.r32(self.emu.teb + 0x2C)
        self.w32(arr + 4 * index, block)
        self.tls_block = block

    # --- stubs -----------------------------------------------------------------------------
    def note(self, name, why):
        s = self.stubs.setdefault(name, {"why": why, "hits": 0})
        s["hits"] += 1

    def arg(self, i):
        """The i-th 4-byte stack argument at a function's entry."""
        return self.emu.arg(i) & 0xFFFFFFFF

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

    def _virtual_alloc(self):
        """VirtualAlloc(addr, size, type, protect): a reservation is a fresh zeroed block of the
        bump heap; a commit inside a known block returns its address."""
        addr, size = self.arg(0), self.arg(1)
        if addr:
            return addr
        a = self.big_alloc((size + 0xFFF) & ~0xFFF, 0x1000)
        self.heap_sizes[a] = size
        return a

    def _slot_alloc(self):
        k = len(self.slots) + 1
        self.slots[k] = 0
        return k

    def _imports(self):
        """Point every IAT slot at a stub of ours: each returns 0 (or a pointer) and pops its
        stdcall arguments, and is logged by name; an import of unknown arity stops the run."""
        d = self.img.d
        e = struct.unpack_from("<I", d, 0x3C)[0]
        irva = struct.unpack_from("<I", d, e + 24 + 96 + 8)[0]
        self.import_names = {}
        self.import_slots = {}   # name -> IAT slot va
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
                ent = struct.unpack_from("<I", d, src + 4 * i)[0]
                if not ent:
                    break
                if ent >> 31:
                    fn = "%s!#%d" % (dll, ent & 0xFFFF)
                elif ent < self.img.size:
                    fn = "%s!%s" % (dll, self.img.cstr(self.B + ent + 2))
                else:
                    fn = "%s!?%#x" % (dll, ent)
                slot = STUBS + 16 * k
                uc.mem_write(slot, b"\xc3")
                self.w32(self.B + ft + 4 * i, slot)
                self.import_names[slot] = fn
                self.import_slots[fn.split("!", 1)[1]] = self.B + ft + 4 * i
                k += 1
                i += 1
            o += 20
        self.import_handlers = {
            "HeapAlloc": lambda vm: vm._heap_alloc(vm.arg(2), vm.arg(1)),
            "HeapReAlloc": lambda vm: vm._heap_realloc(vm.arg(2), vm.arg(3)),
            "HeapFree": lambda vm: 1,
            "HeapSize": lambda vm: vm.heap_sizes.get(vm.arg(2), 0),
            "GetProcessHeap": lambda vm: 0x10000,
            "VirtualAlloc": lambda vm: vm._virtual_alloc(),
            "VirtualFree": lambda vm: 1,
            "FlsAlloc": lambda vm: vm._slot_alloc(),
            "TlsAlloc": lambda vm: vm._slot_alloc(),
            "FlsGetValue": lambda vm: vm.slots.get(vm.arg(0), 0),
            "TlsGetValue": lambda vm: vm.slots.get(vm.arg(0), 0),
            "FlsSetValue": lambda vm: vm.slots.__setitem__(vm.arg(0), vm.arg(1)) or 1,
            "TlsSetValue": lambda vm: vm.slots.__setitem__(vm.arg(0), vm.arg(1)) or 1,
            "EncodePointer": lambda vm: vm.arg(0),
            "DecodePointer": lambda vm: vm.arg(0),
            "InterlockedIncrement": lambda vm: vm._interlocked(1),
            "InterlockedDecrement": lambda vm: vm._interlocked(-1),
            "InterlockedExchange": lambda vm: vm._xchg(),
            "InterlockedCompareExchange": lambda vm: vm._cmpxchg(),
            "InterlockedExchangeAdd": lambda vm: vm._xadd(),
            "GetCurrentThreadId": lambda vm: 0x1000,
            # a virtual wall clock: +1 ms per read, plus what the harness adds per server frame
            # (clock_frame_ms); the VMs' time-based guards (T4/IW3: 2.5 s per execution before
            # "potential infinite loop") read it, so a loop without wait is measured in reads
            "timeGetTime": lambda vm: vm._tick(),
            "GetTickCount": lambda vm: vm._tick(),
        }
        if k:
            uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(self._on_import), begin=STUBS,
                        end=STUBS + 16 * k)

    def _tick(self):
        self.clock_ms += 1
        return self.clock_ms

    def _interlocked(self, delta):
        p = self.arg(0)
        v = (self.r32(p) + delta) & 0xFFFFFFFF
        self.w32(p, v)
        return v

    def _xchg(self):
        p, v = self.arg(0), self.arg(1)
        old = self.r32(p)
        self.w32(p, v)
        return old

    def _cmpxchg(self):
        p, v, cmp = self.arg(0), self.arg(1), self.arg(2)
        old = self.r32(p)
        if old == cmp:
            self.w32(p, v)
        return old

    def _xadd(self):
        p, v = self.arg(0), self.arg(1)
        old = self.r32(p)
        self.w32(p, old + v)
        return old

    def _on_import(self, uc, addr, size, ud):
        fn = self.import_names.get(addr, "?")
        short = fn.split("!", 1)[1]
        if short not in X86_ARGS:
            self.stop_reason = "import of unknown arity called: %s" % fn
            self.note("import " + fn, "unknown arity: stops the run")
            uc.emu_stop()
            return
        h = self.import_handlers.get(short)
        if h:
            r = h(self)
            self.note("import " + fn, "handled")
        else:
            r = 0x10000 if short in PTR_RESULT else 0
            self.note("import " + fn, "returns %s" % ("a pointer" if r else "0"))
        self.emu.ret(r or 0, pop=4 * X86_ARGS[short])

    # --- printf ----------------------------------------------------------------------------
    def format(self, fmt, slot):
        """C printf; slot(k) gives the k-th 4-byte argument slot (a double takes two)."""
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
                width = str(struct.unpack("<i", struct.pack("<I", slot(k)))[0])
                k += 1
            if prec == ".*":
                prec = "." + str(slot(k) & 0x7FFFFFFF)
                k += 1
            spec = "%" + (flags or "") + (width or "") + (prec or "")
            if conv in "fgeEG":
                v = slot(k) | slot(k + 1) << 32
                k += 2
                out.append((spec + conv) % struct.unpack("<d", struct.pack("<Q", v))[0])
                continue
            if length in ("ll", "I64"):
                v = slot(k) | slot(k + 1) << 32
                k += 2
                bits = 64
            else:
                v = slot(k)
                k += 1
                bits = 32
            if conv in "di":
                if v >> (bits - 1):
                    v -= 1 << bits
                out.append((spec + "d") % v)
            elif conv in "uxXo":
                out.append((spec + conv) % v)
            elif conv == "p":
                out.append("%08X" % v)
            elif conv == "c":
                out.append(chr(v & 0xFF))
            elif conv == "s":
                out.append((spec + "s") % (self.cstr(v, 4096) if v else "(null)"))
            else:
                out.append(m.group(0))
        return "".join(out)

    def vformat(self, fmt_va, va_list):
        return self.format(self.cstr(fmt_va, 4096) or "", lambda k: self.r32(va_list + 4 * k))

    def vsnprintf_hook(self):
        """CRT _vsnprintf(buf, size, fmt, va_list) in Python (cdecl)."""
        buf, size, fmt, va = (self.arg(i) for i in range(4))
        b = self.vformat(fmt, va).encode("latin-1", "replace")
        if size:
            n = min(len(b), size - 1)
            self.uc.mem_write(buf, b[:n] + bytes(1))
        return len(b) if len(b) < size else 0xFFFFFFFF

    # --- hooks -----------------------------------------------------------------------------
    def guarded(self, fn):
        """A hook callback whose Python errors stop the emulation; call() re-raises them."""
        def cb(uc, *a):
            try:
                return fn(uc, *a)
            except Exception as ex:  # noqa: BLE001 - reported by call()
                import traceback
                self.hook_error = "%s: %s\n%s" % (type(ex).__name__, ex, traceback.format_exc())
                uc.emu_stop()
                return False
        return cb

    def hook(self, va, fn, name=None, why="", pop=0):
        """Replace the function at va: fn(vm) returns eax (the function is not run); a return
        of False lets the function run after all. pop: bytes a stdcall callee removes."""
        label = name or "%#x" % va

        def cb(uc, addr, size, ud):
            r = fn(self)
            if r is not False:
                self.note(label, why)
                if not self.stop_reason:
                    self.emu.ret(r if r is not None else 0, pop=pop)
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(cb), begin=va, end=va)

    def hook_longjmp(self, va, name="longjmp"):
        """Replace the CRT longjmp(jmp_buf, value) at va by a direct restore of the MSVC x86
        _JUMP_BUFFER (Ebp, Ebx, Edi, Esi, Esp, Eip, Registration): the CRT version runs the SEH
        local unwind first, whose handlers a dump's run never registered."""
        def cb(uc, addr, size, ud):
            buf, value = self.arg(0), self.arg(1)
            ebp, ebx, edi, esi, esp, eip, reg = struct.unpack("<7I", bytes(uc.mem_read(buf, 28)))
            uc.reg_write(X.UC_X86_REG_EBP, ebp)
            uc.reg_write(X.UC_X86_REG_EBX, ebx)
            uc.reg_write(X.UC_X86_REG_EDI, edi)
            uc.reg_write(X.UC_X86_REG_ESI, esi)
            # _setjmp3 saved esp at its own entry (return address on top); the restore returns
            # there: esp after the `ret` of setjmp
            uc.reg_write(X.UC_X86_REG_ESP, esp + 4)
            uc.reg_write(X.UC_X86_REG_EIP, eip)
            uc.reg_write(X.UC_X86_REG_EAX, value or 1)
            self.w32(self.emu.teb, reg)  # fs:[0], the SEH chain at setjmp time
            self.note(name, "direct _JUMP_BUFFER restore (no SEH local unwind)")
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(cb), begin=va, end=va)

    def watch(self, va, fn):
        """Call fn(vm) when the instruction at va runs (the code still runs)."""
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(lambda uc, a, s, u: fn(self)),
                         begin=va, end=va)

    def _on_null_write(self, uc, access, addr, size, value, ud):
        ip = uc.reg_read(X.UC_X86_REG_EIP)
        a, n = self.null_writes.get(ip, (addr, 0))
        self.null_writes[ip] = (a, n + 1)

    def _on_unmapped(self, uc, access, addr, size, value, ud):
        ip = uc.reg_read(X.UC_X86_REG_EIP)
        if access == unicorn.UC_MEM_FETCH_UNMAPPED:
            return False
        if len(self.unmapped) > 512:
            return False
        try:
            uc.mem_map(addr & ~0xFFF, 0x1000)
        except unicorn.UcError:
            return False
        self.unmapped.append((ip, addr))
        return True

    # --- calls -----------------------------------------------------------------------------
    REGS = {"eax": X.UC_X86_REG_EAX, "ebx": X.UC_X86_REG_EBX, "ecx": X.UC_X86_REG_ECX,
            "edx": X.UC_X86_REG_EDX, "esi": X.UC_X86_REG_ESI, "edi": X.UC_X86_REG_EDI}

    def call(self, va, *args, count=50_000_000, **regs):
        """Run the function at va to its return: stack arguments `args` (cdecl/stdcall), register
        arguments by name (eax=..., edi=...). A fault, a hook error or a stop request (set by a
        hook in self.stop_reason) raises Fault."""
        self.hook_error = None
        self.stop_reason = None
        rr = {self.REGS[k]: v & 0xFFFFFFFF for k, v in regs.items()}
        try:
            r = self.emu.call(va, args, count=count, regs=rr)
        except unicorn.UcError as ex:
            raise Fault("%s at %#x (called %#x)\n%s" % (ex, self.emu.ip(), va, self.backtrace()))
        if self.hook_error:
            raise Fault("hook error at %#x: %s" % (self.emu.ip(), self.hook_error))
        if self.stop_reason:
            raise Fault(self.stop_reason)
        if self.emu.ip() != self.emu.retaddr:
            raise Fault("instruction limit (%d) reached at %#x\n%s"
                        % (count, self.emu.ip(), self.backtrace()))
        return r & 0xFFFFFFFF

    def backtrace(self, n=24):
        """Return addresses on the stack that land after a call instruction in the image."""
        sp = self.uc.reg_read(X.UC_X86_REG_ESP)
        out = []
        for k in range(0, 0x4000, 4):
            try:
                v = self.r32(sp + k)
            except unicorn.UcError:
                break
            if self.img.in_image(v) and self.img.is_code(v):
                o = v - self.B
                if self.img.d[o - 5] == 0xE8 or self.img.d[o - 6:o - 4] == b"\xff\x15" \
                        or self.img.d[o - 2] == 0xFF:
                    out.append("  sp+%#x -> %#x" % (k, v))
            if len(out) >= n:
                break
        return "\n".join(out)
