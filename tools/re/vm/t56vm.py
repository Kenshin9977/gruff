"""The Black Ops II (T6) and Black Ops (T5) script VMs run offline under Unicorn, from the 32-bit
executables: the game's own string table, variable system, VM init, global objects, loader and
linker (T6: compiled bytecode from the asset lookup; T5: the engine's own compiler run on the
plain-text script), `Scr_ExecThread`, the frame functions and every opcode handler. Only what the
VM needs from the rest of the engine is stubbed, and every stub is logged.

Addresses are VAs of the executables named by `exe.t6` / `exe.t5mp` (image base 0x400000; PE
timestamps in PROFILES); every name is a hypothesis checked by running, see the comments and
reports/vm_t6.md, vm_t5.md. KisakBlack (an open-source BO1 reimplementation) and the phase-2 RE
gave names to look for; every address here was found and checked in the machine code.

`GameVM32` is the x86 counterpart of vmcore.GameVM (stdcall import stubs with their argument
counts, the 32-bit TLS directory, 4-byte varargs, the x86 _JUMP_BUFFER); vmcore.py is untouched.

Research tool only (Unicorn is GPL-2.0-only, imported from Python like the rest of tools/re).
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn, X  # noqa: E402
from vmcore import Fault, PRINTF  # noqa: E402

BIGHEAP32 = 0x30000000
BIGHEAP32_SZ = 0x20000000   # 512 MiB
STUBS32 = 0x0A000000        # one 16-byte slot per import

# stdcall imports: bytes the callee pops (argument count x 4). An import called with no entry here
# stops the run with its name: returning with a wrong stack would be a silent corruption.
STDCALL_ARGS = {
    "EnterCriticalSection": 1, "LeaveCriticalSection": 1, "InitializeCriticalSection": 1,
    "InitializeCriticalSectionAndSpinCount": 2, "DeleteCriticalSection": 1, "TryEnterCriticalSection": 1,
    "GetCurrentThreadId": 0, "GetCurrentThread": 0, "GetCurrentProcess": 0, "GetCurrentProcessId": 0,
    "GetTickCount": 0, "timeGetTime": 0, "QueryPerformanceCounter": 1, "QueryPerformanceFrequency": 1,
    "Sleep": 1, "SwitchToThread": 0, "InterlockedIncrement": 1, "InterlockedDecrement": 1,
    "InterlockedExchange": 2, "InterlockedCompareExchange": 3, "InterlockedExchangeAdd": 2,
    "HeapAlloc": 3, "HeapReAlloc": 4, "HeapFree": 3, "HeapSize": 3, "GetProcessHeap": 0,
    "VirtualAlloc": 4, "VirtualFree": 3, "VirtualProtect": 4, "VirtualQuery": 3,
    "TlsAlloc": 0, "TlsGetValue": 1, "TlsSetValue": 2, "TlsFree": 1,
    "FlsAlloc": 1, "FlsGetValue": 1, "FlsSetValue": 2, "FlsFree": 1,
    "EncodePointer": 1, "DecodePointer": 1, "GetLastError": 0, "SetLastError": 1,
    "OutputDebugStringA": 1, "IsDebuggerPresent": 0, "SetEvent": 1, "ResetEvent": 1,
    "WaitForSingleObject": 2, "CreateEventA": 4, "CloseHandle": 1, "GetModuleHandleA": 1,
    "GetProcAddress": 2, "CreateFileA": 7, "ReadFile": 5, "WriteFile": 5, "GetFileSize": 2,
    "SetFilePointer": 4, "FindFirstFileA": 2, "FindNextFileA": 2, "FindClose": 1,
    "GetFileAttributesA": 1, "MultiByteToWideChar": 6, "WideCharToMultiByte": 8,
    "GetModuleFileNameA": 3, "GetModuleHandleExA": 3, "GetModuleHandleW": 1,
    "GetProcessTimes": 5, "GetThreadTimes": 5, "GetSystemTimeAsFileTime": 1, "GetLocalTime": 1, "GetSystemTime": 1, "lstrlenA": 1,
}


class GameVM32:
    """A 32-bit game executable under Unicorn: the image at its base, import stubs (logged), the
    module TLS block, a bump heap, a Python printf, `hook`/`watch`/`hook_longjmp`, `call` raising
    Fault."""

    def __init__(self, exe, log=print, verbose=False):
        self.log = log
        self.verbose = verbose
        self.img = img = Image(exe)
        if img.x64:
            raise SystemExit("%s: x64 image, use vmcore.GameVM" % exe)
        self.B = img.base
        self.emu = emu = Emu(img, null_page=True)
        self.uc = uc = emu.uc
        uc.mem_map(BIGHEAP32, BIGHEAP32_SZ)
        self.big_next = BIGHEAP32
        self.stubs = {}
        self.unmapped = []
        self.boot_log = []
        self.hook_error = self.stop_reason = None
        self.slots = {}
        self.heap_sizes = {}
        self._imports()
        uc.hook_add(unicorn.UC_HOOK_MEM_UNMAPPED, self.guarded(self._on_unmapped))
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

    def big_alloc(self, n, align=16):
        a = (self.big_next + align - 1) & ~(align - 1)
        self.big_next = a + n
        if self.big_next > BIGHEAP32 + BIGHEAP32_SZ:
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
            k = min(0x100 - (va & 0xFF), n - len(out))
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
        """IMAGE_TLS_DIRECTORY32: the template copied to a block at
        ThreadLocalStoragePointer[_tls_index] (fs:[0x2c])."""
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

    # --- import stubs ----------------------------------------------------------------------
    def note(self, name, why):
        s = self.stubs.setdefault(name, {"why": why, "hits": 0})
        s["hits"] += 1

    def _heap_alloc(self, n):
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
        d = self.img.d
        e = struct.unpack_from("<I", d, 0x3C)[0]
        irva = struct.unpack_from("<I", d, e + 24 + 96 + 8)[0]
        self.import_names = {}
        uc = self.uc
        uc.mem_map(STUBS32, 0x100000)
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
                elif self.img.in_image(self.B + ent + 2):
                    fn = "%s!%s" % (dll, self.img.cstr(self.B + ent + 2))
                else:
                    fn = "%s!?%#x" % (dll, ent)
                slot = STUBS32 + 16 * k
                uc.mem_write(slot, b"\xc3")
                self.w32(self.B + ft + 4 * i, slot)
                self.import_names[slot] = fn
                k += 1
                i += 1
            o += 20
        a = lambda vm, i: vm.emu.arg(i)  # noqa: E731
        self.import_handlers = {
            "HeapAlloc": lambda vm: vm._heap_alloc(a(vm, 2)),
            "HeapReAlloc": lambda vm: vm._heap_realloc(a(vm, 2), a(vm, 3)),
            "HeapFree": lambda vm: 1,
            "HeapSize": lambda vm: vm.heap_sizes.get(a(vm, 2), 0),
            "GetProcessHeap": lambda vm: 0x10000,
            "TlsAlloc": lambda vm: vm._slot_alloc(),
            "FlsAlloc": lambda vm: vm._slot_alloc(),
            "TlsGetValue": lambda vm: vm.slots.get(a(vm, 0), 0),
            "FlsGetValue": lambda vm: vm.slots.get(a(vm, 0), 0),
            "TlsSetValue": lambda vm: vm.slots.__setitem__(a(vm, 0), a(vm, 1)) or 1,
            "FlsSetValue": lambda vm: vm.slots.__setitem__(a(vm, 0), a(vm, 1)) or 1,
            "EncodePointer": lambda vm: a(vm, 0),
            "DecodePointer": lambda vm: a(vm, 0),
            "GetCurrentThreadId": lambda vm: 1,
            "VirtualAlloc": lambda vm: vm._virtual_alloc(a(vm, 0), a(vm, 1)),
            "VirtualFree": lambda vm: 1,
            "InterlockedIncrement": lambda vm: vm._interlocked(a(vm, 0), 1),
            "InterlockedDecrement": lambda vm: vm._interlocked(a(vm, 0), -1),
            "InterlockedExchange": lambda vm: vm._xchg(a(vm, 0), a(vm, 1)),
            "InterlockedCompareExchange": lambda vm: vm._cmpxchg(a(vm, 0), a(vm, 1), a(vm, 2)),
            "InterlockedExchangeAdd": lambda vm: vm._xadd(a(vm, 0), a(vm, 1)),
        }
        if k:
            uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(self._on_import), begin=STUBS32,
                        end=STUBS32 + 16 * k)

    def _virtual_alloc(self, addr, size):
        """Reserve (address 0) from the bump heap, page aligned; a commit inside a reserved
        range returns its address (the range is already mapped)."""
        if addr:
            return addr
        self.note("VirtualAlloc reserve", "bump heap, never freed")
        return self.big_alloc(max(size, 1), 0x1000)

    def _interlocked(self, p, d):
        v = (self.r32(p) + d) & 0xFFFFFFFF
        self.w32(p, v)
        return v

    def _xchg(self, p, v):
        old = self.r32(p)
        self.w32(p, v)
        return old

    def _cmpxchg(self, p, v, cmp):
        old = self.r32(p)
        if old == cmp:
            self.w32(p, v)
        return old

    def _xadd(self, p, v):
        old = self.r32(p)
        self.w32(p, old + v)
        return old

    def _on_import(self, uc, addr, size, ud):
        fn = self.import_names.get(addr, "?")
        short = fn.split("!", 1)[1]
        if short not in STDCALL_ARGS:
            self.stop_reason = ("import %s called: its stdcall argument count is not known to "
                                "the harness\n%s" % (fn, self.backtrace()))
            uc.emu_stop()
            return
        h = self.import_handlers.get(short)
        if h:
            rax = h(self)
            self.note("import " + fn, "handled")
        else:
            rax = 0
            self.note("import " + fn, "returns 0")
        self.emu.ret(rax or 0, pop=4 * STDCALL_ARGS[short])

    # --- printf ----------------------------------------------------------------------------
    def format(self, fmt, va):
        """C printf over a 32-bit va_list at `va` (4-byte slots; a double takes two)."""
        out = []
        i = 0
        pos = [va]

        def take(n=4):
            v = self.r32(pos[0]) if n == 4 else struct.unpack("<Q", self.uc.mem_read(pos[0], 8))[0]
            pos[0] += n
            return v
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
                width = str(struct.unpack("<i", struct.pack("<I", take()))[0])
            if prec == ".*":
                prec = "." + str(take() & 0x7FFFFFFF)
            spec = "%" + (flags or "") + (width or "") + (prec or "")
            if conv in "fgeEG":
                out.append((spec + conv) % struct.unpack("<d", struct.pack("<Q", take(8)))[0])
                continue
            big = length in ("ll", "I64")
            v = take(8 if big else 4)
            bits = 64 if big else 32
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

    def write_buf(self, buf, size, text):
        b = text.encode("latin-1", "replace")
        if size:
            n = min(len(b), size - 1)
            self.uc.mem_write(buf, b[:n] + bytes(1))
        return len(b) if len(b) < size else -1

    # --- hooks -----------------------------------------------------------------------------
    def guarded(self, fn):
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
        """Replace the function at `va` (cdecl unless `pop`): fn(vm) returns eax (the function is
        not run); a return of False lets the function run after all."""
        label = name or "%#x" % va

        def cb(uc, addr, size, ud):
            r = fn(self)
            if r is not False:
                self.note(label, why)
                self.emu.ret(r if r is not None else 0, pop=pop)
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(cb), begin=va, end=va)

    def hook_longjmp(self, va, name="longjmp"):
        """The CRT longjmp(jmp_buf, value) at `va` replaced by a direct restore of the x86
        _JUMP_BUFFER {Ebp, Ebx, Edi, Esi, Esp, Eip, Registration, ...}: the CRT version unwinds
        the SEH chain (fs:[0], empty here) through RtlUnwind."""
        def cb(uc, addr, size, ud):
            buf, value = self.emu.arg(0), self.emu.arg(1)
            for off, r in ((0, X.UC_X86_REG_EBP), (4, X.UC_X86_REG_EBX), (8, X.UC_X86_REG_EDI),
                           (12, X.UC_X86_REG_ESI), (16, X.UC_X86_REG_ESP), (20, X.UC_X86_REG_EIP)):
                uc.reg_write(r, self.r32(buf + off))
            uc.reg_write(X.UC_X86_REG_EAX, value or 1)
            self.note(name, "direct _JUMP_BUFFER restore (no SEH unwind)")
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(cb), begin=va, end=va)

    def watch(self, va, fn):
        self.uc.hook_add(unicorn.UC_HOOK_CODE, self.guarded(lambda uc, a, s, u: fn(self)),
                         begin=va, end=va)

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
    def call(self, va, *args, count=50_000_000, regs=None):
        """Run the function at `va` to its return (cdecl: every argument on the stack; `regs`
        for a usercall's register arguments). A fault, a hook error or a stop request raises
        Fault."""
        self.hook_error = None
        self.stop_reason = None
        try:
            r = self.emu.call(va, args, count=count, regs=regs)
        except unicorn.UcError as ex:
            raise Fault("%s at %#x (called %#x)\n%s" % (ex, self.emu.ip(), va, self.backtrace()))
        if self.hook_error:
            raise Fault("hook error at %#x: %s" % (self.emu.ip(), self.hook_error))
        if self.stop_reason:
            raise Fault(self.stop_reason)
        if self.emu.ip() != self.emu.retaddr:
            raise Fault("instruction limit (%d) reached at %#x\n%s"
                        % (count, self.emu.ip(), self.backtrace()))
        return r

    def backtrace(self, n=24):
        sp = self.uc.reg_read(X.UC_X86_REG_ESP)
        out = []
        for k in range(0, 0x4000, 4):
            try:
                v = self.r32(sp + k)
            except unicorn.UcError:
                break
            if self.img.in_image(v) and self.img.is_code(v):
                o = v - self.B
                if self.img.d[o - 5] == 0xE8 or self.img.d[o - 6:o - 4] == b"\xff\x15":
                    out.append("  sp+%#x -> %#x" % (k, v))
            if len(out) >= n:
                break
        return "\n".join(out)


# --- per-game anchors (VAs, image base 0x400000) ----------------------------------------------
PROFILES = {
    "t6": {
        "exe_key": "exe.t6",
        "timestamp": 1786983181,
        "title": "Black Ops II",
        # SL_Init(): MT_Init (0x7df080: the node pool 0x2b45300, its pointer at 0x2b44e24) and the
        # string hash table (0xc000 buckets, pointer at 0xcb0a00, static). Com_Init's only call
        "SL_Init": 0x7E3890,
        # Scr_InitVariables(inst): four pools from PMem_Alloc (0x75b860) into scrVarGlob
        # (0x2d3c480 + inst * 0x200 ...). Com_Init (0x6c8236) calls it, then Scr_Init
        "Scr_InitVariables": 0x7E4D60,
        # Scr_Init(inst): Scr_VM_Init (0x7eaaf0: stack, frames, the "sv_clientside" dvar), then
        # bInited (gScrVarPub+0x30)
        "Scr_Init": 0x7EAC20,
        # Scr_Settings(inst, developer, developer_script, abort_on_error): Com_SetScriptSettings
        # (0x6c44d0) calls it for both instances; gScrVarPub +4 / +5, scrVmPub abort +0x15
        "Scr_Settings": 0x7EAC80,
        # G_InitGame (0x5ff910): Scr_InitSystem? (0x7ec340: time array, pause array, level and
        # the free entity list objects), Scr_SetLoading(inst, 1) (0x7ead10), Scr_AllocGameVariable
        # (0x7e76f0)
        "Scr_InitSystem": 0x7EC340,
        "Scr_SetLoading": 0x7EAD10,
        "Scr_AllocGameVariable": 0x7E76F0,
        # GScr_LoadScripts (0x622ca0): Scr_BeginLoadScripts(inst, 1) (0x7de700,
        # "Scr_BeginLoadScripts" hunk), Scr_LoadScript(inst, name) (0x7de2f0 -> 0x7de120:
        # DB_FindXAssetHeader(0x30, "<name>.gsc"), header check 0x7df4e0, link 0x7e0f00),
        # Scr_GetFunctionHandle(inst, file, func, 0, 1) (0x7dfef0); then 0x7dfab0 (loads
        # zone_source/<lang>/scriptgdb.clump: debug info, not run here) and
        # Scr_EndLoadScripts(inst) (0x7de300)
        "Scr_BeginLoadScripts": 0x7DE700,
        "Scr_LoadScript": 0x7DE2F0,
        "Scr_GetFunctionHandle": 0x7DFEF0,
        "Scr_EndLoadScripts": 0x7DE300,
        # DB_FindXAssetHeader(type, name, errorIfMissing, waitTime): hooked; ScriptParseTree is
        # type 0x30 = {name, len, buffer}
        "DB_FindXAssetHeader": 0x59A120, "asset_scriptparsetree": 0x30,
        # Scr_ExecThread(inst, handle, paramcount) -> thread id (u16); Scr_FreeThread(inst, id):
        # the StartGameType callback runner (0x639990)
        "Scr_ExecThread": 0x7F1970,
        "Scr_FreeThread": 0x7EC310,
        # G_RunFrame (0x6015b0): Scr_RunCurrentThreads(0) (0x7f2810: VM_SetTime 0x7f1b60, the
        # threads due at gScrVarPub.time) before the entity loop, Scr_IncTime(0) (0x7f2950: the
        # runner again, free entity list, time += 1) after it
        "Scr_RunCurrentThreads": 0x7F2810,
        "Scr_IncTime": 0x7F2950,
        # Scr_Error(inst, msg, terminal) (0x7eccf0) stores the message (gScrVarPub+8) and the
        # terminal byte (scrVmPub+0x16), then Scr_ErrorInternal (0x7ead30, inst in eax): longjmp
        # (0xa28540) into VM_Execute's recovery, or "Terminal script error"
        "Scr_ErrorInternal": 0x7EAD30,
        "longjmp": 0xA28540,
        # PMem_Alloc(size, align, ?, pool, name, track): bump heap
        "PMem_Alloc": 0x75B860,
        # CRT _vsnprintf(buf, size, fmt, va_list) (0xa230ad): Com_sprintf (0x75d300) and va
        # (0x75d410, per-thread ring buffers from Sys_GetValue) go through it
        "vsnprintf": 0xA230AD, "va": 0x75D410,
        # CRT sprintf(buf, fmt, ...) (0xa23292): its locale data (_getptd, TLS) is not set up
        "sprintf": 0xA23292,
        # Com_Printf(channel, fmt, ...), Com_PrintError(channel, fmt, ...) (0x6c2790),
        # Com_Error(code, fmt, ...)
        "Com_Printf": 0x6C2670, "Com_PrintError": 0x6C2790, "Com_Error": 0x6C2A50,
        # functions replaced by `return 0`: LoadScriptGDB (0x7dfc50: opens
        # zone_source/<lang>/scriptgdb.clump for the line numbers of a link error; reached only
        # from the link error reporter 0x7e0720, which prints "line 0" without it)
        "returns_0": {"LoadScriptGDB": 0x7DFC50},
        # data
        "gScrVarPub": 0x2D3C880, "gScrVarPub_stride": 0x54,
        # gScrVarPub: +4 developer, +5 developer_script, +8 error_message, +0x10 time, +0x14
        # timeArrayId, +0x18 pauseArrayId, +0x1c levelId (Scr_InitSystem's objects), +0x30 bInited
        "error_message": 0x8, "time": 0x10, "levelId": 0x1C,
        # scrVmPub[inst] (0x2d3c950, stride 0x42a8): top +0x10, inparamcount +0x18,
        # outparamcount +0x1c, terminal_error +0x16 (read off Scr_VM_Init and Scr_GetInt
        # 0x7f1c00); values are {u32 type, u32 u} (type first)
        "scrVmPub": 0x2D3C950, "scrVmPub_stride": 0x42A8,
        "vm": {"top": 0x10, "outparamcount": 0x1C, "terminal": 0x16},
        "value_type_off": 0, "value_off": 4,
        # string table nodes of 24 bytes ({u32 refcount/user, text}) at [0x2b44e24]
        "sl_pool_ptr": 0x2B44E24, "sl_node": 24, "sl_text": 4,
        "types": ("undefined object string localized_string vector hash float int codepos precodepos "
                  "function stack animation developer_codepos thread notify_thread time_thread "
                  "child_thread struct removed_entity entity array removed_thread free"),
        # logprint's handler (gruff-data builtins/t6.toml, rva 0x228bf0)
        "logprint": 0x628BF0,
        # AllocThread(inst, self) (0x7e5900): an object of type 0xe (thread) with self at +2;
        # called by Scr_ExecThread (0x7f19cb), the two other Scr_Exec*Thread and the thread-call
        # opcodes of VM_Execute (0x7ed7e0)
        "AllocThread": 0x7E5900,
        # ... except this one: the EndOn opcode (0x57, handler 0x7ef62c) allocates a thread-typed
        # object (self = the running frame) as the endon's record in the object's event list;
        # counted as a thread it renumbered every later thread (measured on multiple_endons)
        "AllocThread_not_threads": (0x7EF673,),
        # object variables: [0x2d3c480 + inst * 0x200], 8-byte entries {u16 refs, u16 self, u32
        # type word} (AllocObject 0x7e5760: free entries have type 0x17)
        "objects": {"array": 0x2D3C480, "inst_stride": 0x200, "size": 8, "type_off": 4,
                    "type_mask": 0x7F},
        # VM_Execute's frame state gFs[inst] (0x2d45f88, stride 0x14): pos, top, localId,
        # localVarCount, startTop (loaded at VM_Execute's entry 0x7ed817; localId measured: the
        # AllocThread result of the running thread)
        "gfs": {"addr": 0x2D45F88, "stride": 0x14, "localId": 8},
        # the order of the boot calls (G_InitGame's after Com_Init's) and the argument templates
        "boot": ("SL_Init", "Scr_InitVariables", "Scr_Init", "Scr_Settings", "Scr_InitSystem",
                 "Scr_SetLoading", "Scr_AllocGameVariable"),
        "end_load": ("Scr_EndLoadScripts", "Scr_SetLoading"),
        "sig": {"SL_Init": (), "Scr_Settings": ("inst", "dev", "dev", 0),
                "Scr_InitSystem": ("inst", 1), "Scr_SetLoading": ("inst", "loading"),
                "Scr_BeginLoadScripts": ("inst", 1), "Scr_LoadScript": ("inst", "name"),
                "Scr_GetFunctionHandle": ("inst", "name", "func", 0, 1),
                "Scr_ExecThread": ("inst", "handle", 0), "Scr_FreeThread": ("inst", "tid")},
    },
    "t5": {
        # Black Ops multiplayer: the install's BlackOpsMP.exe (not encrypted, file layout). The SP
        # exe (exe.t5) has no logprint, the trace builtin
        "exe_key": "exe.t5mp",
        "timestamp": 1320282179,
        "title": "Black Ops",
        # Com_Init (0x86cf10): SL_Init(0) and SL_Init(1) (0x5049e0, 0x86cfa3), ...,
        # Scr_InitVariables(0) (0x5cd480: PMem_Alloc of the 0x7dffc8-byte variable list, the
        # KisakBlack size), Scr_Init(0) (0x4ecee0: class map 0x61b950, Scr_VM_Init 0x8e5b20 with
        # the "sv_clientside" dvar, bInited), Scr_Settings(developer, developer_script, abort,
        # inst) (0x55d010, twice)
        "SL_Init": 0x5049E0,
        "Scr_InitVariables": 0x5CD480,
        "Scr_Init": 0x4ECEE0,
        "Scr_Settings": 0x55D010,
        # G_InitGame (0x5f6c90, 0x5f715b): Scr_InitSystem(0, 1) (0x65fe00: time array, pause
        # array, level, anim, time = 0), Scr_SetLoading(1, 0) (0x5fab60), Scr_AllocGameVariable(0)
        # (0x4bff40)
        "Scr_InitSystem": 0x65FE00,
        "Scr_SetLoading": 0x5FAB60,
        "Scr_AllocGameVariable": 0x4BFF40,
        # GScr_LoadScripts (0x41fb20): Scr_BeginLoadScripts(inst, 1) (0x624dc0, the
        # "Scr_BeginLoadScripts" hunk), Scr_LoadScript(inst, name) (0x6b6120 ->
        # Scr_LoadScriptInternal 0x560cf0: "%s.gsc", Scr_AddSourceBuffer 0x62fdc0 -> Scr_ReadFile
        # 0x8e37a0 -> the fast-file reader 0x8e35d0: DB_FindXAssetHeader(0x24 rawfile), a .gsc
        # buffer used as plain text; then the game's own parser and compiler),
        # Scr_GetFunctionHandle(inst, file, func) (0x5d5600: an offset into the program buffer),
        # Scr_EndLoadScripts(inst) (0x59c7e0)
        "Scr_BeginLoadScripts": 0x624DC0,
        "Scr_LoadScript": 0x6B6120,
        "Scr_GetFunctionHandle": 0x5D5600,
        "Scr_EndLoadScripts": 0x59C7E0,
        "DB_FindXAssetHeader": 0x493A60, "asset_scriptparsetree": 0x24,
        # Scr_ReadFile picks the fast-file reader when the dvar at [0x359eb00] (useFastFile:
        # current value byte at +0x18) is set; no dvar is registered in an executable at rest
        "fake_dvars": {0x359EB00: 1},
        "rawfile_zlib": True,
        # Scr_InitClassMap (0x61b950) starts with a call to 0xa0bbd0 (2 callers), an SEH-guarded
        # routine outside the script VM (0xa0b320: a value from 0x573790, a file opened with
        # CreateFile-style flags) that ends in RaiseException and resumes in its own handler;
        # it returns 1. Replaced by `return 1`
        "returns_1": {"seh_probe_0xa0bbd0": 0xA0BBD0},
        # Scr_ExecThread(inst, handle, paramcount) (0x69bb00: AllocThread 0x43ca60, VM_Execute
        # 0x8eade0), Scr_FreeThread(id, inst) (0x5c17c0): the StartGameType runner (0x4efaf0)
        "Scr_ExecThread": 0x69BB00,
        "Scr_FreeThread": 0x5C17C0,
        # frames: Scr_RunCurrentThreads(inst) (0x41c180: VM_SetTime 0x8eb040) and Scr_IncTime(inst)
        # (0x6bea70: VM_SetTime, the free entity list, time += 1); both called by the frame
        # function 0x5c9ea0 (0x5ca10b, 0x5ca197) around its entity loop
        "Scr_RunCurrentThreads": 0x41C180,
        "Scr_IncTime": 0x6BEA70,
        # Scr_Error(inst, msg, terminal) (0x6245e0) -> Scr_ErrorInternal (0x8e5c30, inst in eax):
        # longjmp (0x9d05c4) to the jmp_buf 0x3dd3998 + (inst * 33 + depth) * 64
        "Scr_ErrorInternal": 0x8E5C30,
        "longjmp": 0x9D05C4,
        "PMem_Alloc": 0x4C58C0,
        # CRT _vsnprintf (0x9cae40) and va (0x502ca0), read off the phase-2 extraction
        "vsnprintf": 0x9CAE40, "va": 0x502CA0,
        # Com_Printf(channel, fmt, ...) (0x4126c0, Com_Init's banner), Com_Error(code, fmt, ...)
        # (0x627380, the gametype loader's "Could not find script")
        "Com_Printf": 0x4126C0, "Com_Error": 0x627380,
        "gScrVarPub": 0x3DCB280, "gScrVarPub_stride": 0x58,
        # gScrVarPub (KisakBlack's scrVarPub_t, offsets checked against Scr_VM_Init and
        # Scr_InitSystem): +6 developer, +0xc error_message, +0x14 time, +0x18 timeArrayId, +0x20
        # levelId, +0x34 bInited
        "error_message": 0xC, "time": 0x14, "levelId": 0x20,
        # scrVmPub (0x3dcb338, stride 0x4320): top +0x10, terminal_error +0x16, outparamcount
        # +0x1c (Scr_VM_Init 0x8e5b20); values are {u32 u, u32 type} (stack[0].type = 7 at +0x324)
        "scrVmPub": 0x3DCB338, "scrVmPub_stride": 0x4320,
        "vm": {"top": 0x10, "outparamcount": 0x1C, "terminal": 0x16},
        "value_type_off": 4, "value_off": 0,
        # string table per instance: [0x3b6a800 + inst * 4], 16-byte nodes, text at +4
        # (SL_ConvertToString 0x624c70)
        "sl_pool_ptr": 0x3B6A800, "sl_per_inst": 1, "sl_node": 16, "sl_text": 4,
        "types": ("undefined object string localized_string vector float int codepos precodepos "
                  "function stack animation developer_codepos thread notify_thread time_thread "
                  "child_thread struct removed_entity entity array removed_thread free"),
        # logprint: GSC function table 0xbf49d8, entry 222 {name, handler, developer}
        "logprint": 0x842670,
        # AllocThread(inst, self) (0x43ca60): status 0x6d (in use | type 13 thread)
        "AllocThread": 0x43CA60,
        # the EndOn opcode's endon record (0x8e99b9), as in T6
        "AllocThread_not_threads": (0x8E99B9,),
        # variables: [0x3dcb180 + inst * 0x80], 28-byte entries, id n at entry n + 1 (refcount u16
        # +8, u +0xc, status +0x10: type in the low 5 bits)
        "objects": {"array": 0x3DCB180, "inst_stride": 0x80, "size": 28, "index_base": 1,
                    "type_off": 0x10, "type_mask": 0x1F},
        # gFs[inst] (0x3dd4a28, stride 0x14): pos, localId, localVarCount, top, startTop
        # (VM_Execute 0x8e6b90 loads them at its entry)
        "gfs": {"addr": 0x3DD4A28, "stride": 0x14, "localId": 4,
                # ... but VM_Execute keeps the running frame in its own locals and writes gFs
                # back only when it leaves (measured: a new thread's first logprint read the
                # caller's id). Its dispatch (0x8e6c16, `movzx ecx, byte [ebx]`, every opcode)
                # is watched instead and fs.localId read from the local copy at [ebp + 0x5c]
                # (fs at ebp+0x58: pos, localId, localVarCount, top, startTop; ebp = esp - 0x74
                # at the entry 0x8e6b90)
                "dispatch": 0x8E6C16, "ebp_localId": 0x5C},
        # Hunk_InitMemory() (0x6177a0, Com_Init 0x86d154): the hunk the compiler's temporary
        # memory comes from (5 MB with useFastFile set, through VirtualAlloc)
        "Hunk_InitMemory": 0x6177A0,
        # Hunk_InitDebugMemory() (0x68d790, Com_Init's next call 0x86d159): the 23 MB debug
        # hunk ([0x397d278]) developer mode's Scr_InitOpcodeLookup (0x64fe50) allocates from
        "Hunk_InitDebugMemory": 0x68D790,
        # CRT malloc (0x9cf4d6): zlib's inflate allocates through it (Z_Malloc 0x436c50); the
        # CRT heap is not set up in an executable at rest (it ends in LoadLibraryA)
        "malloc": 0x9CF4D6,
        # CRT sscanf (0x9cdbbe): the lexer reads float literals with "%f" (0x8ebcb8)
        "sscanf": 0x9CDBBE,
        "boot": ("Hunk_InitMemory", "Hunk_InitDebugMemory", "SL_Init", "Scr_InitVariables", "Scr_Init", "Scr_Settings",
                 "Scr_InitSystem", "Scr_SetLoading", "Scr_AllocGameVariable"),
        "end_load": ("Scr_EndLoadScripts", "Scr_SetLoading"),
        "sig": {"Hunk_InitMemory": (), "Hunk_InitDebugMemory": (), "Scr_Settings": ("dev", "dev", 0, "inst"),
                "Scr_InitSystem": ("inst", 1), "Scr_SetLoading": ("loading", "inst"),
                "Scr_BeginLoadScripts": ("inst", 1), "Scr_LoadScript": ("inst", "name"),
                "Scr_GetFunctionHandle": ("inst", "name", "func"),
                "Scr_ExecThread": ("inst", "handle", 0), "Scr_FreeThread": ("tid", "inst")},
    },
}


class T56VM(GameVM32):
    def __init__(self, game, exe, log=print, verbose=False, developer=False):
        super().__init__(exe, log, verbose)
        self.game = game
        self.P = P = PROFILES[game]
        if self.img.timestamp != P["timestamp"]:
            raise SystemExit("%s: PE timestamp %d, the profile is for %d"
                             % (exe, self.img.timestamp, P["timestamp"]))
        self.inst = 0
        self.developer = developer
        self.trace = []
        self.errors = []
        self.messages = []
        self.thread_names = {}
        self.thread_count = 0
        self.step_count = 20_000_000
        self.frames_run = 0
        self.assets = {}
        self.asset_requests = []
        self.vt_names = dict(enumerate(P["types"].split()))
        self.vt = {v: k for k, v in self.vt_names.items()}

    # --- addresses ---------------------------------------------------------------------------
    def pub(self, off):
        return self.P["gScrVarPub"] + self.P["gScrVarPub_stride"] * self.inst + off

    def vmpub(self, field):
        return self.P["scrVmPub"] + self.P["scrVmPub_stride"] * self.inst + self.P["vm"][field]

    # --- stubs ---------------------------------------------------------------------------------
    def install_stubs(self):
        P = self.P
        self.hook(P["PMem_Alloc"], lambda vm: vm.big_alloc(vm.emu.arg(0), max(vm.emu.arg(1), 16)),
                  "PMem_Alloc", "bump heap, never freed")
        self.hook(P["vsnprintf"], T56VM._vsnprintf, "_vsnprintf", "Python formatter")
        self.hook(P["va"], T56VM._va, "va", "Python formatter, a fresh buffer per call")
        if "malloc" in P:
            self.hook(P["malloc"], lambda vm: vm._heap_alloc(vm.emu.arg(0)), "malloc",
                      "bump heap, never freed")
        if "sprintf" in P:
            self.hook(P["sprintf"], T56VM._sprintf, "sprintf", "Python formatter")
        self.hook_longjmp(P["longjmp"])
        self.hook(P["DB_FindXAssetHeader"], T56VM._find_asset, "DB_FindXAssetHeader",
                  "returns the test scripts' ScriptParseTree; any other asset: not found")
        self.hook(P["Com_Printf"], lambda vm: vm._print(1), "Com_Printf", "message recorded")
        if "Com_PrintError" in P:
            self.hook(P["Com_PrintError"], lambda vm: vm._print(1, "error: "), "Com_PrintError",
                      "message recorded")
        self.hook(P["Com_Error"], T56VM._com_error, "Com_Error",
                  "message recorded, the run stops with it")
        for name, va in P.get("returns_0", {}).items():
            self.hook(va, lambda vm: 0, name, "returns 0 at once (see the profile)")
        for name, va in P.get("returns_1", {}).items():
            self.hook(va, lambda vm: 1, name, "returns 1 at once (see the profile)")
        if "sscanf" in P:
            self.hook(P["sscanf"], T56VM._sscanf, "sscanf", "Python: %f %d %i %x only")

    def _sscanf(self):
        """CRT sscanf(text, fmt, ...) for the conversions the lexer uses; any other format stops
        the run (a silent wrong parse would be worse)."""
        import re as _re
        text = self.cstr(self.emu.arg(0), 4096) or ""
        fmt = self.cstr(self.emu.arg(1), 256) or ""
        convs = _re.findall(r"%([a-z]+)", fmt)
        if not convs or any(c not in ("f", "d", "i", "x") for c in convs) or len(convs) > 1:
            self.stop_reason = "sscanf format not handled by the harness: %r" % fmt
            self.uc.emu_stop()
            return 0
        c, out = convs[0], self.emu.arg(2)
        m = _re.match(r"\s*([-+]?(\d+\.?\d*([eE][-+]?\d+)?|\.\d+([eE][-+]?\d+)?))", text) if c == "f" \
            else _re.match(r"\s*([-+]?(0[xX][0-9a-fA-F]+|\d+))", text)
        if not m:
            return 0
        if c == "f":
            self.uc.mem_write(out, struct.pack("<f", float(m.group(1))))
        else:
            self.w32(out, int(m.group(1), 16 if c == "x" else 0) if c != "d" else int(m.group(1)))
        return 1

    def _vsnprintf(self):
        buf, size, fmt, va = (self.emu.arg(i) for i in range(4))
        return self.write_buf(buf, size, self.format(self.cstr(fmt, 4096) or "", va))

    def _sprintf(self):
        buf, fmt = self.emu.arg(0), self.cstr(self.emu.arg(1), 4096) or ""
        sp = self.uc.reg_read(X.UC_X86_REG_ESP)
        return self.write_buf(buf, 1 << 20, self.format(fmt, sp + 12))

    def _va(self):
        fmt = self.cstr(self.emu.arg(0), 4096) or ""
        sp = self.uc.reg_read(X.UC_X86_REG_ESP)
        return self.put_cstr(self.format(fmt, sp + 8)[:0x3FF])

    def _print(self, fmt_arg, prefix=""):
        sp = self.uc.reg_read(X.UC_X86_REG_ESP)
        fmt = self.cstr(self.emu.arg(fmt_arg), 4096) or ""
        text = self.format(fmt, sp + 4 + 4 * (fmt_arg + 1)).strip()
        if text:
            self.messages.append((self.now(), prefix + text))
        return 0

    def _com_error(self):
        sp = self.uc.reg_read(X.UC_X86_REG_ESP)
        code = self.emu.arg(0)
        text = self.format(self.cstr(self.emu.arg(1), 4096) or "", sp + 12).strip()
        self.messages.append((self.now(), "Com_Error %d: %s" % (code, text)))
        self.stop_reason = "Com_Error %d: %s" % (code, text)
        self.uc.emu_stop()
        return 0

    @staticmethod
    def asset_key(name):
        return (name or "").replace("\\", "/").lower()

    def _find_asset(self):
        t, name = self.emu.arg(0), self.cstr(self.emu.arg(1), 512)
        self.asset_requests.append((t, name))
        if t == self.P["asset_scriptparsetree"]:
            return self.assets.get(self.asset_key(name), 0)
        return 0

    def add_script(self, name, data):
        """ScriptParseTree {name, len, buffer} for `name` (with its .gsc extension); the lookup
        matches names case-insensitively, either slash."""
        data = self.strip_name_prefix(data)
        if self.P.get("rawfile_zlib"):
            # T5: the fast-file reader (0x8e35d0) inflates a .gsc/.csc rawfile: {u32 length,
            # u32 compressed length, zlib stream} (uncompress 0x678310); our plain-text source is
            # stored that way, NUL-terminated as the compiler reads it
            import zlib
            c = zlib.compress(data + b"\0")
            data = struct.pack("<II", len(data) + 1, len(c)) + c
        buf = self.big_alloc(len(data) + 16, 16)
        self.uc.mem_write(buf, data)
        spt = self.big_alloc(16, 16)
        self.w32(spt, self.put_cstr(name))
        self.w32(spt + 4, len(data))
        self.w32(spt + 8, buf)
        self.assets[self.asset_key(name)] = spt
        return spt

    @staticmethod
    def strip_name_prefix(data, prefix=b"t6" + bytes([92])):
        """gsc-tool writes a T6 object's own name (header +0x30) with its output folder in front
        ("t6" + backslash + "maps..."): the prefix is cut (the string moved left, NUL padded) so
        that the game's messages name the script as the asset lookup does. Logged by the caller."""
        if data[:4] != b"\x80GSC":
            return data
        o = struct.unpack_from("<H", data, 0x30)[0]
        if data[o:o + len(prefix)] != prefix:
            return data
        end = data.index(b"\0", o)
        d = bytearray(data)
        d[o:end] = data[o + len(prefix):end] + bytes(len(prefix))
        return bytes(d)

    # --- boot and load -------------------------------------------------------------------------
    def pcall(self, key, count=None, **kw):
        """Call the profile's function `key` with its argument template (P["sig"]): "inst", a
        keyword given here, or a constant."""
        tmpl = self.P["sig"].get(key, ("inst",))
        args = [self.inst if a == "inst" else kw[a] if isinstance(a, str) else a for a in tmpl]
        return self.call(self.P[key], *args, count=count or self.step_count)

    def boot(self):
        P = self.P
        self.install_stubs()
        for va, value in P.get("fake_dvars", {}).items():
            dv = self.big_alloc(0x80, 16)
            self.w8(dv + 0x18, value)
            self.w32(va, dv)
            self.note("dvar %#x" % va, "a fake dvar whose current value byte (+0x18) is %d" % value)
        dev = 1 if self.developer else 0
        for key in P["boot"]:
            self.pcall(key, dev=dev, loading=1)

    def begin_load(self):
        # T6: its first call (0x7e05c0) builds the canonical name list (2580 names, a quadratic
        # de-duplication: about 200M instructions)
        self.pcall("Scr_BeginLoadScripts", count=1_000_000_000)

    def load_script(self, name):
        """Scr_LoadScript(inst, name without extension): nonzero when loaded."""
        return self.pcall("Scr_LoadScript", name=self.put_cstr(name), count=self.step_count * 5)

    def function_handle(self, name, func):
        return self.pcall("Scr_GetFunctionHandle", name=self.put_cstr(name),
                          func=self.put_cstr(func))

    def end_load(self):
        for key in self.P["end_load"]:
            self.pcall(key, dev=0, loading=0)

    # --- values ------------------------------------------------------------------------------------
    def now(self):
        return self.r32(self.pub(self.P["time"])) & 0xFFFFFF

    def sl_string(self, idx):
        if not idx:
            return ""
        pool = self.r32(self.P["sl_pool_ptr"] + 4 * self.inst * self.P.get("sl_per_inst", 0))
        return self.cstr(pool + self.P["sl_node"] * idx + self.P["sl_text"], 4096)

    def value_text(self, va):
        t = self.r32(va + self.P["value_type_off"])
        v = self.r32(va + self.P["value_off"])
        VT = self.vt
        if t == VT["undefined"]:
            return "undefined"
        if t in (VT["string"], VT["localized_string"]):
            return self.sl_string(v & 0xFFFF)
        if t == VT["int"]:
            return str(struct.unpack("<i", struct.pack("<I", v))[0])
        if t == VT["float"]:
            return "%g" % struct.unpack("<f", struct.pack("<I", v))[0]
        if t == VT["vector"]:
            x, y, z = struct.unpack("<fff", bytes(self.uc.mem_read(v, 12)))
            return "(%g, %g, %g)" % (x, y, z)
        return "<%s %#x>" % (self.vt_names.get(t, "type%d" % t), v)

    def builtin_args(self):
        n = self.r32(self.vmpub("outparamcount"))
        top = self.r32(self.vmpub("top"))
        return [self.value_text(top - 8 * i) for i in range(n)]

    # --- threads ---------------------------------------------------------------------------------
    def obj_flags(self, idx):
        """The type word of object variable `idx`: low 7 bits the type, a child thread's parent
        local id from bit 8 (AllocChildThread writes (parent << 8) | type)."""
        T = self.P["objects"]
        base = self.r32(T["array"] + T["inst_stride"] * self.inst)
        return self.r32(base + T["size"] * (idx + T.get("index_base", 0)) + T["type_off"])

    def current_thread(self):
        """The running thread: gFs.localId is the current function's frame; a script call runs in
        a child_thread object whose parent is its caller's frame; the thread is the root."""
        F = self.P["gfs"]
        if "dispatch" in F:
            v = self.cur_local & 0xFFFF
        else:
            v = self.r32(F["addr"] + F["stride"] * self.inst + F["localId"]) & 0xFFFF
        child = self.vt["child_thread"]
        for _ in range(256):
            fl = self.obj_flags(v)
            if fl & self.P["objects"]["type_mask"] != child:
                return v
            v = (fl >> 8) & 0xFFFF
        return v

    def track_threads(self):
        """Name threads T0, T1... in creation order: the return of AllocThread at each of its
        direct call sites (the thread-call opcodes and Scr_Exec*Thread)."""
        F = self.P["gfs"]
        self.cur_local = 0
        if "dispatch" in F:
            def on_op(vm):
                vm.cur_local = vm.r32(vm.uc.reg_read(X.UC_X86_REG_EBP) + F["ebp_localId"])
            self.watch(F["dispatch"], on_op)
        va = self.P["AllocThread"]
        skip = set(self.P.get("AllocThread_not_threads", ()))
        sites = [s for s in self.img.rel32_callers({va})[va] if s[0] not in skip]

        def on(vm):
            tid = vm.uc.reg_read(X.UC_X86_REG_EAX) & 0xFFFF
            vm.thread_names[tid] = "T%d" % vm.thread_count
            vm.thread_count += 1
        for site, kind in sites:
            if kind == "call":
                self.watch(site + 5, on)
        self.boot_log.append("AllocThread call sites watched: %d" % len(sites))

    def thread_name(self, tid):
        return self.thread_names.get(tid, "thread%d" % tid)

    def trace_builtin(self, va, label):
        def on(vm):
            vm.trace.append((vm.now(), vm.thread_name(vm.current_thread()), label,
                             vm.builtin_args()))
            return 0
        self.hook(va, on, "builtin " + label, "recorded in the trace, handler not run")

    def trace_errors(self, flood=64):
        """Each runtime error at Scr_ErrorInternal's entry (after Scr_Error stored the message
        and the terminal byte): (time, thread, message, terminal, trace position)."""
        def on(vm):
            msg = vm.cstr(vm.r32(vm.pub(vm.P["error_message"])), 1024) or ""
            term = bool(vm.r8(vm.vmpub("terminal")))
            vm.errors.append((vm.now(), vm.thread_name(vm.current_thread()), msg, term,
                              len(vm.trace)))
            if len(vm.errors) >= flood:
                vm.stop_reason = "error flood: %d script errors" % len(vm.errors)
                vm.uc.emu_stop()
        self.watch(self.P["Scr_ErrorInternal"], on)

    # --- running -------------------------------------------------------------------------------
    def exec_thread(self, handle):
        """Scr_ExecThread(inst, handle, 0) (self = level), then Scr_FreeThread of its id, as the
        engine's callback runners do."""
        tid = self.pcall("Scr_ExecThread", handle=handle) & 0xFFFF
        self.pcall("Scr_FreeThread", tid=tid)
        return tid & 0xFFFF

    def run_frame(self):
        """One server frame as G_RunFrame drives the VM: Scr_RunCurrentThreads, (the entity
        loop, empty here), Scr_IncTime (the runner again, then time += 1)."""
        self.frames_run += 1
        self.pcall("Scr_RunCurrentThreads")
        self.pcall("Scr_IncTime")
