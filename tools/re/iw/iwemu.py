"""Unicorn harness for the IW extraction, x86 and x64: the image at its base, a stack, a fake
TEB (x64: gs base; x86: an fs segment through a GDT), a heap, pages mapped on demand (a
null-plus-offset read continues on zeros), and call(va, args) that runs one function to its
return. Pattern from graywolf-tools zonekit/emulator.py and tools/re/emu.py (T7), extended to
32-bit. Self-contained until the shared tools/re module exists.
"""
import struct

from image import user_site

user_site()
import unicorn  # noqa: E402
from unicorn import x86_const as X  # noqa: E402
from common.emu import MemoryAccess, gdt_entry, write_teb  # noqa: E402

RET64 = 0x7C0000000000
STACK64 = 0x7F0000000000
TEB64 = 0x7E0000000000
HEAP64 = 0x7D0000000000
RET32 = 0xEE000000
STACK32 = 0xE0000000
TEB32 = 0xED000000
GDT32 = 0xEC000000
HEAP32 = 0xD0000000
STACK_SZ = 0x200000
HEAP_SZ = 0x4000000


class Stop(Exception):
    pass


class Emu(MemoryAccess):
    def __init__(self, img, low_zero=True):
        self.img = img
        self.x64 = img.x64
        uc = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_64 if self.x64 else unicorn.UC_MODE_32)
        size = (img.size + 0xFFFF) & ~0xFFFF
        uc.mem_map(img.base, size, unicorn.UC_PROT_ALL)
        uc.mem_write(img.base, img.mem)
        if self.x64:
            self.RET, self.STACK, self.HEAP, teb = RET64, STACK64, HEAP64, TEB64
        else:
            self.RET, self.STACK, self.HEAP, teb = RET32, STACK32, HEAP32, TEB32
        uc.mem_map(self.STACK, STACK_SZ, unicorn.UC_PROT_READ | unicorn.UC_PROT_WRITE)
        uc.mem_map(teb, 0x20000)
        uc.mem_map(self.HEAP, HEAP_SZ, unicorn.UC_PROT_READ | unicorn.UC_PROT_WRITE)
        uc.mem_map(self.RET, 0x1000)
        uc.mem_write(self.RET, b"\xf4" * 0x1000)  # hlt: never run past the sentinel
        self.returned = False

        def _at_ret(uc_, addr, size, data):
            self.returned = True
            uc_.emu_stop()
        uc.hook_add(unicorn.UC_HOOK_CODE, _at_ret, begin=self.RET, end=self.RET)
        write_teb(uc, teb, self.x64)
        if not self.x64:
            uc.mem_write(teb + 0x0, struct.pack("<I", 0xFFFFFFFF))  # SEH chain end
            uc.mem_map(GDT32, 0x1000)
            g = (b"\0" * 8 + gdt_entry(0, 0xFFFFF, 0x9A, 0xC) + gdt_entry(0, 0xFFFFF, 0x92, 0xC)
                 + gdt_entry(teb, 0xFFFF, 0x92, 0x4))
            uc.mem_write(GDT32, g)
            uc.reg_write(X.UC_X86_REG_GDTR, (0, GDT32, len(g) - 1, 0))
            uc.reg_write(X.UC_X86_REG_CS, 1 << 3)
            for r in (X.UC_X86_REG_DS, X.UC_X86_REG_ES, X.UC_X86_REG_SS, X.UC_X86_REG_GS):
                uc.reg_write(r, 2 << 3)
            uc.reg_write(X.UC_X86_REG_FS, 3 << 3)
        if low_zero:
            # null + offset reads continue on zeros; never executable (a jump to 0 stops)
            uc.mem_map(0, 0x100000, unicorn.UC_PROT_READ | unicorn.UC_PROT_WRITE)
        # code read+exec only (a runaway write into code faults instead of corrupting it; the
        # obfuscated IW8 dump's few self-patching handlers fault there), data read+write only
        for name, va, vsz, ch in img.sections:
            a = img.base + (va & ~0xFFF)
            n = ((va + vsz + 0xFFF) & ~0xFFF) - (va & ~0xFFF)
            if ch & 0x20000000:
                uc.mem_protect(a, n, unicorn.UC_PROT_READ | unicorn.UC_PROT_EXEC)
            else:
                uc.mem_protect(a, n, unicorn.UC_PROT_READ | unicorn.UC_PROT_WRITE)
        self.uc = uc
        self.teb = teb
        # dirty pages: restored after every run, so each run starts from the pristine image
        self.dirty = set()
        self.demand_pages = set()
        self.patches = {}       # va -> bytes, re-applied when their page is restored
        self.patch_pages = {}
        if low_zero:
            # a call through a null function pointer returns at once: a `ret` at address 0
            uc.mem_protect(0, 0x1000, unicorn.UC_PROT_ALL)
            self.patch(0, b"\xc3")

        def _on_write(uc_, access, addr, size, value, data):
            self.dirty.add(addr & ~0xFFF)
            if (addr + size - 1) & ~0xFFF != addr & ~0xFFF:
                self.dirty.add((addr + size - 1) & ~0xFFF)
        # only the image and the low region need restoring (stack writes stay in C)
        uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, _on_write, begin=img.base, end=img.base + size - 1)
        uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, _on_write, begin=0, end=0xFFFFF)
        self.write_hook_all = None

        def _on_write_demand(uc_, access, addr, size, value, data):
            p = addr & ~0xFFF
            if p in self.demand_pages:
                self.dirty.add(p)
        self._on_write_demand = _on_write_demand
        self.heap0 = self.heap_next = self.HEAP
        self.on_demand = 0
        # static TLS: the TEB's slot array (x64 gs:[0x58], x86 fs:[0x2c]) points every
        # module's slot at its own zeroed block, so __declspec(thread) data is addressable
        slots = self.alloc(64 * self.ptr_size(), 64)
        for i in range(64):
            blk = self.alloc(0x10000, 0x1000)
            uc.mem_write(slots + i * self.ptr_size(),
                         struct.pack("<Q" if self.x64 else "<I", blk))
        if self.x64:
            uc.mem_write(teb + 0x58, struct.pack("<Q", slots))
        else:
            uc.mem_write(teb + 0x2C, struct.pack("<I", slots))
        uc.hook_add(unicorn.UC_HOOK_MEM_READ_UNMAPPED | unicorn.UC_HOOK_MEM_WRITE_UNMAPPED,
                    self._unmapped)
        self.foreign_calls = 0

        def _fetch(uc_, access, addr, size, value, data):
            # a call through an import slot a dump left resolved (a system DLL address): map a
            # page of `ret` there, so the call returns (rax unspecified)
            # (also an unresolved slot holding its hint RVA, a low address outside the image)
            if self.x64 and addr >= 0x100000 and not (img.base <= addr < img.base + size_) and not (
                    self.HEAP <= addr < self.HEAP + HEAP_SZ or self.STACK <= addr < self.STACK + STACK_SZ):
                page = addr & ~0xFFF
                try:
                    uc_.mem_map(page, 0x1000, unicorn.UC_PROT_READ | unicorn.UC_PROT_EXEC)
                    uc_.mem_write(page, b"\xc3" * 0x1000)
                except unicorn.UcError:
                    return False
                self.foreign_calls += 1
                return True
            return False
        size_ = size
        uc.hook_add(unicorn.UC_HOOK_MEM_FETCH_UNMAPPED, _fetch)
        self.sp_reg = X.UC_X86_REG_RSP if self.x64 else X.UC_X86_REG_ESP
        self.ip_reg = X.UC_X86_REG_RIP if self.x64 else X.UC_X86_REG_EIP
        self.ax_reg = X.UC_X86_REG_RAX if self.x64 else X.UC_X86_REG_EAX
        self.ptr = 8 if self.x64 else 4

    def _unmapped(self, uc, access, addr, size, value, data):
        # map a zero page for plausible data addresses (heap-like, low); not for code fetches
        page = addr & ~0xFFF
        if self.on_demand > 4000:
            return False
        if self.x64:
            if not (0x10000 <= addr < 0x7FFFFFFFFFFF):
                return False
        else:
            if not (0x10000 <= addr < 0xFFFFF000):
                return False
        try:
            uc.mem_map(page, 0x1000)
        except unicorn.UcError:
            return False
        self.on_demand += 1
        self.demand_pages.add(page)
        return True

    def restore(self):
        """Undo every write since the last restore: image pages from the image, the low zero
        region and on-demand pages to zeros, the TEB (with its TLS slot array) to its state
        after setup. The heap and stack are the caller's."""
        img, uc = self.img, self.uc
        if not hasattr(self, "_teb_saved"):
            self._teb_saved = bytes(uc.mem_read(self.teb, 0x20000))

            def _teb_w(uc_, access, addr, size, value, data):
                self._teb_dirty = True
            uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, _teb_w, begin=self.teb, end=self.teb + 0x1FFFF)
            uc.ctl_flush_tb()
            self._teb_dirty = False
        elif self._teb_dirty:
            uc.mem_write(self.teb, self._teb_saved)
            self._teb_dirty = False
        z = bytes(0x1000)
        for p in self.dirty:
            if img.base <= p < img.base + img.size:
                o = p - img.base
                uc.mem_write(p, img.mem[o:o + 0x1000].ljust(0x1000, b"\x00"))
                for pv in self.patch_pages.get(p, ()):
                    uc.mem_write(pv, self.patches[pv])
            elif p < 0x100000:
                uc.mem_write(p, z)
                for pv in self.patch_pages.get(p, ()):
                    uc.mem_write(pv, self.patches[pv])
        self.dirty.clear()
        # pages mapped on demand go away: the next run maps them again, zeroed
        for p in self.demand_pages:
            try:
                uc.mem_unmap(p, 0x1000)
            except unicorn.UcError:
                pass
        self.demand_pages.clear()
        # allocations made during a run (HeapAlloc stubs) are dropped with it
        if getattr(self, "heap_mark", None):
            self.heap_next = self.heap_mark
        if hasattr(self, "tls_vals"):
            self.tls_vals.clear()  # the per-thread data lived on the dropped heap
        self.on_demand = 0

    def patch(self, va, data):
        self.uc.mem_write(va, data)
        self.patches[va] = data
        self.patch_pages.setdefault(va & ~0xFFF, []).append(va)
        if (va + len(data) - 1) & ~0xFFF != va & ~0xFFF:
            self.patch_pages.setdefault((va + len(data) - 1) & ~0xFFF, []).append(va)

    # stdcall argument counts of the x86 imports the engine code reaches (a stub must pop them)
    X86_ARGS = {
        "GetLastError": 0, "SetLastError": 1, "EnterCriticalSection": 1, "LeaveCriticalSection": 1,
        "TryEnterCriticalSection": 1, "InterlockedIncrement": 1, "InterlockedDecrement": 1,
        "InterlockedExchange": 2, "InterlockedCompareExchange": 3, "InterlockedExchangeAdd": 2,
        "timeGetTime": 0, "GetTickCount": 0, "QueryPerformanceCounter": 1,
        "QueryPerformanceFrequency": 1, "GetCurrentThreadId": 0, "GetCurrentThread": 0,
        "GetCurrentProcess": 0, "GetCurrentProcessId": 0, "Sleep": 1, "SleepEx": 2,
        "TlsGetValue": 1, "TlsSetValue": 2, "TlsAlloc": 0, "WaitForSingleObject": 2,
        "SetEvent": 1, "ResetEvent": 1, "GetProcessHeap": 0, "HeapAlloc": 3, "HeapFree": 3,
        "HeapReAlloc": 4, "HeapSize": 3, "OutputDebugStringA": 1, "IsDebuggerPresent": 0,
        "GetModuleHandleA": 1, "GetSystemTimeAsFileTime": 1, "GetLocalTime": 1,
        "GetSystemTime": 1, "MultiByteToWideChar": 6, "WideCharToMultiByte": 8,
        "SwitchToThread": 0, "FlsGetValue": 1, "FlsSetValue": 2, "EncodePointer": 1,
        "DecodePointer": 1, "RaiseException": 4, "RtlUnwind": 4, "GetStringTypeA": 5,
        "GetStringTypeW": 4, "LCMapStringA": 6, "LCMapStringW": 6, "GetCPInfo": 2, "GetACP": 0,
        "GetOEMCP": 0, "IsValidCodePage": 1, "VirtualAlloc": 4, "VirtualFree": 3,
        "GetFileAttributesA": 1, "CloseHandle": 1, "ReleaseSemaphore": 3,
        "WaitForMultipleObjects": 4, "GetModuleFileNameA": 3, "GetModuleHandleW": 1,
        "GetFileAttributesW": 1, "OutputDebugStringW": 1,
    }
    # imports whose result must be a usable pointer, not 0
    PTR_RESULT = {"HeapAlloc", "HeapReAlloc", "GetProcessHeap",
                  "VirtualAlloc", "DecodePointer", "EncodePointer", "GetModuleHandleA",
                  "GetCurrentThread", "GetCurrentProcess"}
    # imports that end a run: the script error path's unwinding
    STOP = {"RaiseException", "RtlUnwind", "RtlUnwindEx", "RtlRaiseException", "longjmp",
            "_longjmp", "ExitProcess", "TerminateProcess"}

    def install_import_stubs(self):
        """Point every IAT slot at a stub that returns 0 (or a pointer to zeros), popping the
        stdcall arguments on x86. An x86 import of unknown arity, or an unwinding import, stops
        the run (self.stop_reason says which)."""
        imps = self.img.imports()
        base = 0x7B0000000000 if self.x64 else 0xEB000000
        n = max(1, len(imps))
        self.uc.mem_map(base, (n * 16 + 0xFFF) & ~0xFFF)
        self.uc.mem_write(base, b"\xf4" * (n * 16))
        self.imp_by_stub = {}
        for i, (slot, dll, nm) in enumerate(imps):
            st = base + 16 * i
            self.imp_by_stub[st] = (dll, nm)
            self.patch(slot, struct.pack("<Q" if self.x64 else "<I", st))
        self.zero_page = self.alloc(0x10000, 0x1000)
        self.tls_vals = {}
        self.stop_reason = None

        def on_stub(uc_, addr, size, data):
            dll, nm = self.imp_by_stub.get(addr, ("?", "?"))
            if nm in self.STOP:
                self.stop_reason = "import %s" % nm
                uc_.emu_stop()
                return
            val = self.zero_page if nm in self.PTR_RESULT else 0
            if nm in ("HeapAlloc", "VirtualAlloc", "HeapReAlloc"):
                val = self.alloc(0x2000)
            if nm in ("TlsGetValue", "TlsSetValue", "FlsGetValue", "FlsSetValue"):
                # thread-local slots keep what was stored (the CRT allocates its per-thread
                # data once and reads it back; a slot that always reads 0 loops for ever)
                if self.x64:
                    a0, a1 = uc_.reg_read(X.UC_X86_REG_RCX), uc_.reg_read(X.UC_X86_REG_RDX)
                else:
                    sp = uc_.reg_read(X.UC_X86_REG_ESP)
                    a0, a1 = self.r32(sp + 4), self.r32(sp + 8)
                if nm.endswith("SetValue"):
                    self.tls_vals[a0] = a1
                    val = 1
                else:
                    val = self.tls_vals.get(a0, 0)
            if nm in ("EncodePointer", "DecodePointer"):  # identity
                val = (uc_.reg_read(X.UC_X86_REG_RCX) if self.x64
                       else self.r32(uc_.reg_read(X.UC_X86_REG_ESP) + 4))
            if self.x64:
                self.do_return(val)
            else:
                k = self.X86_ARGS.get(nm)
                if k is None:
                    self.stop_reason = "import %s (arity unknown)" % nm
                    uc_.emu_stop()
                    return
                self.do_return(val, 4 * k)
        self.uc.hook_add(unicorn.UC_HOOK_CODE, on_stub, begin=base, end=base + 16 * n)

    def ptr_size(self):
        return 8 if self.x64 else 4

    def setup_call(self, va, args=(), regs=None):
        """Registers and stack for a call to va: x64 Windows ABI (rcx, rdx, r8, r9 + shadow),
        x86 cdecl (args pushed). `regs` sets extra registers (custom conventions)."""
        uc = self.uc
        sp = self.STACK + STACK_SZ - 0x2000
        if self.x64:
            for r, v in zip((X.UC_X86_REG_RCX, X.UC_X86_REG_RDX, X.UC_X86_REG_R8, X.UC_X86_REG_R9), args):
                uc.reg_write(r, v)
            sp -= 0x28
            for i, v in enumerate(args[4:]):
                uc.mem_write(sp + 0x28 + 8 * i, struct.pack("<Q", v))
            uc.mem_write(sp, struct.pack("<Q", self.RET))
        else:
            for v in reversed(list(args)):
                sp -= 4
                uc.mem_write(sp, struct.pack("<I", v & 0xFFFFFFFF))
            sp -= 4
            uc.mem_write(sp, struct.pack("<I", self.RET))
        # flags reset: a probed function may leave the trap flag set (popfq), which would
        # single-step every later run into an exception
        uc.reg_write(X.UC_X86_REG_RFLAGS if self.x64 else X.UC_X86_REG_EFLAGS, 0x202)
        for r, v in (regs or {}).items():
            uc.reg_write(r, v)
        uc.reg_write(self.sp_reg, sp)
        uc.reg_write(X.UC_X86_REG_RBP if self.x64 else X.UC_X86_REG_EBP, sp + 0x400)
        return sp

    def run(self, va, count=0, timeout_us=0):
        """Run from va; self.returned says whether the sentinel return address was reached."""
        self.returned = False
        self.uc.emu_start(va, self.RET, timeout=timeout_us, count=count)
        if self.uc.reg_read(self.ip_reg) == self.RET:
            self.returned = True

    def call(self, va, args=(), count=0, timeout_us=0, regs=None):
        self.setup_call(va, args, regs)
        self.run(va, count, timeout_us)
        return self.uc.reg_read(self.ax_reg)

    def unpatch(self, va, original):
        """Drop a patch: the original bytes back, and no longer re-applied."""
        self.uc.mem_write(va, original)
        data = self.patches.pop(va, None)
        if data is not None:
            for p in {va & ~0xFFF, (va + len(data) - 1) & ~0xFFF}:
                lst = self.patch_pages.get(p, [])
                if va in lst:
                    lst.remove(va)

    def stub_crt_ptd(self, log=None):
        """The static CRT's per-thread data getter (_getptd_noexit) cannot work in a process the
        CRT never initialised (its TLS index and its encoded FLS pointers are garbage): it
        loops. It is recognised by its opening, a GetLastError call in its first bytes, among
        the functions starting so the most called one; stubbed to return a zeroed block."""
        imps = {nm: slot for slot, dll, nm in self.img.imports()}
        gle = imps.get("GetLastError")
        if not gle or self.x64:
            return None
        pat = b"\xff\x15" + struct.pack("<I", gle)
        img = self.img
        starts = []
        i = img.mem.find(pat)
        while i >= 0:
            # a function whose GetLastError call sits in its first 4 bytes (after push esi/edi)
            for back in (1, 2, 3, 4):
                f = img.base + i - back
                if all(b in (0x56, 0x57, 0x53, 0x55) for b in img.mem[i - back:i]):
                    if img.mem[i - back - 1] in (0xCC, 0x90, 0xC3, 0xC2):
                        starts.append(f)
                        break
            i = img.mem.find(pat, i + 1)
        if not starts:
            return None
        from common.image import rel32_branches
        cnt = {}
        for sva, vsz, op, t in rel32_branches(img.mem, img.base, img.sections, tail=5):
            for s in starts:
                cnt[s] = cnt.get(s, 0) + int(((op == 0xE8) & (t == s)).sum())
        best = max(cnt, key=lambda s: cnt[s])
        blk = self.alloc(0x1000, 64)
        self.stub(best, blk)
        if log:
            log("crt: _getptd_noexit 0x%x (%d callers) stubbed to a zeroed block" % (best, cnt[best]))
        return best

    def stub(self, va, value=0):
        """Every later call to va returns `value` at once (cdecl / x64: no argument popped)."""
        def _h(uc_, addr, size, data):
            self.do_return(value)
        self.uc.hook_add(unicorn.UC_HOOK_CODE, _h, begin=va, end=va)
        self.uc.ctl_flush_tb()

    def do_return(self, value=0, pop_extra=0):
        """From a hook at a function's entry: return `value` to the caller (rax/eax, xmm0 = 0)."""
        uc = self.uc
        sp = uc.reg_read(self.sp_reg)
        ret = self.rptr(sp)
        uc.reg_write(self.ax_reg, value)
        uc.reg_write(self.sp_reg, sp + self.ptr + pop_extra)
        uc.reg_write(self.ip_reg, ret)
