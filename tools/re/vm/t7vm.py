"""The Black Ops III (T7) script VM run offline under Unicorn, from the flat dump: the game's own
init, loader, linker, scheduler and opcode handlers; only what the VM needs from the rest of the
engine is stubbed, and every stub is logged (`T7VM.stubs`).

This is a research tool, not part of the shipped toolchain: it imports Unicorn (GPL-2.0-only)
from Python, like the rest of tools/re, and writes data files (traces, facts). Nothing here is
linked into the Rust crates.

Addresses are RVAs of the dump named by `exe.t7` (PE timestamp 1765634846). Every name is a
hypothesis checked by running: see ADDR's comments and reports/vm_t7.md.
"""
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn, X  # noqa: E402
from common.hashes import t7_canon  # noqa: E402

# --- addresses (RVA) -------------------------------------------------------------------------
# Names come from boiii-free's symbols (another build: code at -0x20, data at +0x7f000 from
# this dump) and KisakBlack's VM; each one is accepted only by what it does when run here.
ADDR = {
    # PMem-style allocator (size, align, ?, pool, ...): ScrVar_Init allocates through it
    "alloc": 0x20C5FB0,
    # ScrVar_Init(inst): allocates the variable pool (130000/65000 x 0x40) and the name-search
    # hash list (0x40000), then tail-calls ScrVar_InitVariables
    "ScrVar_Init": 0x12DF230,
    # Scr_Init(inst): 0x12cc5a0 (loader state; stores the gscc magic 0x1c000a0d43534780),
    # 0x12df320 (class map), 0x12ed870 (VM pub: stack, frames), then gScrVarPub.bInited = 1
    "Scr_Init": 0x12EC2E0,
    # timeArrayId, pauseArrayId, world, classes, level, anim (+ game when 0): the global objects
    "Scr_InitObjects": 0x12EC340,
    # DB_FindXAssetHeader(type, name, errorIfMissing, waitTime): Scr_LoadScript's asset lookup
    "DB_FindXAssetHeader": 0x1420EF0,
    "Scr_LoadScript": 0x12C8410,
    # MT_Init(): points gScrMemTreeGlob (0x4cdce80) at the static node pool and head table
    "MT_Init": 0x12C8F00,
    # Scr_BeginLoadScripts(inst, user) / Scr_EndLoadScripts(inst): GScr_LoadScripts (0x1aa2530)
    # brackets its Scr_LoadScript calls with them
    "Scr_BeginLoadScripts": 0x12C7E10,
    "Scr_EndLoadScripts": 0x12C7F40,
    # Scr_GetFunctionHandle(inst, script, ns, func, 0, 1): GScr_LoadScripts' next call
    "Scr_GetFunctionHandle": 0x12CC110,
    # Scr_ExecThread(inst, codepos, paramcount, retval*, self): 68 callers; AllocThread(self or
    # level) then VM_ExecuteInternal (0x12edda0)
    "Scr_ExecThread": 0x12EA790,
    # loads <script>.gdb from scriptgdb.zip for debug info (file system): stubbed
    "LoadScriptGDB": 0x12CB120,
    # Scr_RunCurrentThreads(inst) then gScrVarPub.time += 1: one server frame of the VM
    "Scr_RunFrame": 0x12EC2A0,
    "Scr_RunCurrentThreads": 0x12ECC30,
    "VM_Execute": 0x12EDB30,
    # Scr_Error(inst, msg, terminal): keeps the first message in gScrVarPub.error_message (+0x10),
    # stores terminal at scrVmPub+0x2b, then jumps to the throw (0x12ea4e0, "throwing script
    # exception"), which longjmps back into VM_Execute's error recovery
    "Scr_Error": 0x12EA450,
    "Scr_Throw": 0x12EA4E0,
    # printf-like error print (channel, ?, fmt, ...): Scr_LoadScript reports a missing asset with it
    "Com_PrintError": 0x20EC0B0,
    # CRT vsnprintf(buf, size, fmt, va_list): replaced by a Python formatter (the CRT's locale
    # and per-thread data are not initialised in a dump)
    "vsnprintf": 0x2BC44C0,
    # CRT sprintf(buf, fmt, ...): the link error reporter formats line numbers with it
    "sprintf": 0x2BC3FB0,
    # va(fmt, ...): rotating per-thread buffers reached through a thread value the dump lacks
    "va": 0x227D160,
    # Com_PrintfV(channel, fmt, va_list): under Com_Printf (0x20f0c20, channel 0x36)
    "Com_PrintfV": 0x20F0720,
}
ASSET_SCRIPTPARSETREE = 0x36
FNV_IMUL = re.compile(rb"\x69[\xc0-\xff]\x93\x01\x00\x01")
PRINTF = re.compile(r"%([-+ #0]*)(\*|\d+)?(\.\*|\.\d+)?(hh|h|ll|l|I64|I32|z|j|t|L)?([diouxXeEfgGcspn%])")
ERROR_FLOOD = 64  # script errors in one run before the harness stops it

# ScrVar_AllocThread's call sites in the ScriptThreadCall/ScriptMethodThreadCall handlers (and
# their pointer and class variants) and in the four Scr_Exec*Thread APIs
ALLOC_THREAD_SITES = (0x12CF3D6, 0x12CF5E3, 0x12CF7B3, 0x12CF9B6, 0x12CFB7E, 0x12EA809, 0x12EA905,
                      0x12EA9F7, 0x12EAAB8)
# data
GVAR_GLOB = 0x5124500     # gScrVarGlob[inst] stride 0x100: +0x80 variable pool
GVAR_PUB = 0x5124700      # gScrVarPub[inst] stride 0x78: +0x18 time, +0x1c timeArrayId, +0x2c levelId
GVM_PUB = 0x5124840       # scrVmPub[inst] stride 0x8a40: +0x20 top, +0x34 inparamcount
VM_FS = 0x513DD30         # the VM's current function_state[inst] stride 0x20: pos, top, localId at +0x10
SL_POOL = 0x505D2D0       # pointer to the memory-tree node pool that SL strings live in (SL_ConvertToString)
VAR_SIZE = 0x40
# value types (boiii-free's ScrVarType order; int = 7 measured by the PoF)
VT_NAMES = dict(enumerate(
    "undefined pointer string istring vector hash float int uint64 uintptr entity_offset "
    "codepos precodepos api_function function stack animation thread notify_thread "
    "time_thread child_thread class struct removed_entity entity array removed_thread free "
    "thread_list ent_list".split()))
VT = {v: k for k, v in VT_NAMES.items()}
PUB_FIELDS = {"time": 0x18, "timeArrayId": 0x1C, "pauseArrayId": 0x20, "worldId": 0x24,
              "classesId": 0x28, "levelId": 0x2C, "gameId": 0x30, "animId": 0x34,
              "freeEntList": 0x38, "tempVariable": 0x3C, "bInited": 0x40}

BIGHEAP = 0x7B0000000000
BIGHEAP_SZ = 0x20000000   # 512 MiB
STUBS = 0x7A0000000000    # one 16-byte slot per import


class Fault(Exception):
    pass


def builtin_table_ranges(timestamp):
    """(rva, size) of each builtin table, from phase 2's out/t7_extract.json for this build."""
    import json
    p = os.path.join(os.path.dirname(HERE), "out", "t7_extract.json")
    if not os.path.exists(p):
        raise SystemExit("needs %s: run `py -3.11 -I treyarch/t7_builtins.py extract` first" % p)
    with open(p) as f:
        d = json.load(f)
    if d["pe_timestamp"] != timestamp:
        raise SystemExit("%s is for PE timestamp %d, the dump is %d" % (p, d["pe_timestamp"],
                                                                       timestamp))
    return sorted({(t["rva"], 0x20 * t["count"]) for lk in d["lookups"].values()
                   for t in lk["tables"]})


class T7VM:
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
        self.trace = []
        self.thread_names = {}
        self.thread_count = 0
        self.errors = []
        self.on_thread_alloc = []  # callbacks (thread id, name), after a thread is named
        self.on_error = []         # callbacks (message, terminal), where the VM throws
        self.step_count = 20_000_000
        self.frame_model = "g_runframe"  # or "end_only": Scr_RunFrame alone
        self.frame = 0
        self.hook_error = self.stop_reason = None
        self.asset_requests = []
        self._imports()
        uc.hook_add(unicorn.UC_HOOK_MEM_UNMAPPED, self.guarded(self._on_unmapped))
        self.hook(ADDR["vsnprintf"], T7VM._vsnprintf, "vsnprintf", "Python formatter")
        self.hook(ADDR["sprintf"], T7VM._sprintf, "sprintf", "Python formatter")
        self.hook(ADDR["va"], T7VM._va, "va", "Python formatter, a fresh buffer per call")
        self.prints = []
        self.hook(ADDR["Com_PrintfV"], T7VM._printfv, "Com_PrintfV", "message recorded")
        self._tls()

    def _tls(self):
        """The module's thread-local block, copied from the TLS template, at
        ThreadLocalStoragePointer[_tls_index] (MT_Init and the profiler read it)."""
        d = self.img.d
        e = struct.unpack_from("<I", d, 0x3C)[0]
        trva, tsz = struct.unpack_from("<II", d, e + 24 + 112 + 8 * 9)
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
    # --- memory ----------------------------------------------------------------------------
    def r32(self, va):
        return self.emu.r32(va)

    def r64(self, va):
        return self.emu.r64(va)

    def w32(self, va, v):
        self.emu.w32(va, v)

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

    def _heap_alloc(self, n, flags=0):
        a = self.big_alloc(max(n, 1), 16)
        self.heap_sizes[a] = n
        if flags & 8:  # HEAP_ZERO_MEMORY (the big heap is zero already; reuse never happens)
            pass
        return a

    def _slot_alloc(self):
        k = len(self.slots) + 1
        self.slots[k] = 0
        return k

    def _heap_realloc(self, old, n):
        a = self._heap_alloc(n)
        if old:
            k = min(n, self.heap_sizes.get(old, 0))
            self.uc.mem_write(a, bytes(self.uc.mem_read(old, k)))
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

    def put_cstr(self, s):
        b = s.encode("latin-1") + b"\0"
        a = self.big_alloc(len(b), 16)
        self.uc.mem_write(a, b)
        return a

    # --- stubs -----------------------------------------------------------------------------
    def note(self, name, why):
        s = self.stubs.setdefault(name, {"why": why, "hits": 0})
        s["hits"] += 1

    def _imports(self):
        """Point every import slot at a stub of ours: each returns 0 unless a handler says
        otherwise, and is logged by name when called."""
        d = self.img.d
        e = struct.unpack_from("<I", d, 0x3C)[0]
        dd = e + 24 + 112
        irva = struct.unpack_from("<I", d, dd + 8)[0]
        self.import_names = {}
        uc = self.uc
        uc.mem_map(STUBS, 0x100000)
        k = 0
        o = irva
        while True:
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
                else:
                    fn = "%s!%s" % (dll, self.img.cstr(self.B + (ent & 0x7FFFFFFF) + 2))
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
            # per-thread slots of the CRT (vsnprintf's locale data lives there)
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
            wide = length in ("ll", "I64", "z", "j", "t")
            bits = 64 if wide else 32
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

    def _vsnprintf(self):
        buf, size, fmt, va = (self.emu.arg(i) for i in range(4))
        text = self.format(self.cstr(fmt, 4096) or "", lambda k: self.r64(va + 8 * k))
        b = text.encode("latin-1", "replace")
        if size:
            n = min(len(b), size - 1)
            self.uc.mem_write(buf, b[:n] + bytes(1))
        return len(b) if len(b) < size else -1

    def _sprintf(self):
        buf, fmt = self.emu.arg(0), self.emu.arg(1)
        text = self.format(self.cstr(fmt, 4096) or "", lambda k: self.emu.arg(2 + k))
        b = text.encode("latin-1", "replace")
        self.uc.mem_write(buf, b + bytes(1))
        return len(b)

    def _va(self):
        fmt = self.emu.arg(0)
        text = self.format(self.cstr(fmt, 4096) or "", lambda k: self.emu.arg(1 + k))
        return self.put_cstr(text[:0x3FF])

    def _printfv(self):
        ch, fmt, va = self.emu.arg(0), self.emu.arg(1), self.emu.arg(2)
        text = self.format(self.cstr(fmt, 4096) or "", lambda k: self.r64(va + 8 * k))
        self.prints.append((self.now(), ch & 0xFFFFFFFF, text))
        if self.verbose:
            self.log("[print %#x] %s" % (ch & 0xFFFFFFFF, text.rstrip()))
        if "Com_ERROR:" in text:
            # Com_Error prints its banner, then longjmps to a frame the dump's run never set:
            # stop here and report it
            self.stop_reason = "Com_Error: " + text.split("Com_ERROR:", 1)[1].strip()
            self.uc.emu_stop()
        return 0

    def hook(self, rva, fn, name=None, why=""):
        """Replace the function at rva: fn(vm) returns rax (the function is not run)."""
        va = self.B + rva
        label = name or "%#x" % rva

        def cb(uc, addr, size, ud):
            r = fn(self)
            if r is not False:
                self.note(label, why)
                self.emu.ret(r if r is not None else 0)
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

    # --- boot and load ---------------------------------------------------------------------
    def boot(self, inst=0, developer=False, builtins=True):
        """The VM's own init, in the order the engine's Com/SV init calls it."""
        self.inst = inst
        self.hook(ADDR["alloc"], lambda vm: vm.big_alloc(vm.emu.arg(0), max(vm.emu.arg(1), 16)),
                  "PMem_Alloc", "bump heap, never freed")
        self.hook(ADDR["LoadScriptGDB"], lambda vm: 0, "LoadScriptGDB",
                  "no debug zip (scriptgdb.zip is read from the file system)")
        self.hook(ADDR["DB_FindXAssetHeader"], T7VM._find_asset, "DB_FindXAssetHeader",
                  "returns the test scripts' ScriptParseTree; any other asset: not found")
        self.assets = {}
        self.script_buffers = {}
        if builtins:
            self._builtin_tables()
        self.call(ADDR["MT_Init"])
        self.call(ADDR["ScrVar_Init"], inst)
        self.call(ADDR["Scr_Init"], inst)
        self.call(ADDR["Scr_InitObjects"], inst)
        # gScrVarPub.developer (+8): retail runs with 0; 1 is the mod tools' developer mode (the
        # throw then takes the debugger path: a terminal error becomes Com_Error)
        self.uc.mem_write(self.B + GVAR_PUB + 0x78 * inst + 8, bytes([1 if developer else 0]))

    def _builtin_tables(self):
        """The builtin tables are built by static initialisers (phase 2's FNV pattern): run each
        once, then keep the .data pages they changed (and the heap they used) in a local cache
        under tools/re/out (derived from the game: never committed), keyed by PE timestamp."""
        import zlib
        cache = os.path.join(os.path.dirname(HERE), "out",
                             "t7vm_static_init_%d.bin" % self.img.timestamp)
        if os.path.exists(cache):
            with open(cache, "rb") as f:
                raw = zlib.decompress(f.read())
            o = 0
            n = 0
            while o < len(raw):
                va, ln = struct.unpack_from("<QI", raw, o)
                o += 12
                if va == 0:  # the heap ends after the initialisers
                    self.emu.heap_next, self.big_next = struct.unpack_from("<QQ", raw, o)
                else:
                    self.uc.mem_write(va, raw[o:o + ln])
                o += ln
                n += 1
            self.boot_log.append("static init: %d regions from the cache %s"
                                 % (n, os.path.basename(cache)))
            return
        # the real function head (a .pdata entry can be a chunk in the middle of a function)
        funcs = sorted({self.img.head_of(self.B + m.start())
                        for m in FNV_IMUL.finditer(self.img.d, 0, self.img.texts[0][1] - self.B)}
                       - {None})
        heap0, big0 = self.emu.heap_next, self.big_next
        ok = 0
        for f in funcs:
            try:
                self.emu.call(f, (), count=5_000_000)
                ok += 1
            except unicorn.UcError:
                pass
        # Keep only the builtin tables (their places come from phase 2's lookup traces,
        # out/t7_extract.json, made from this same dump); every other byte the initialisers
        # wrote (dvars, canonical names with pointers into a heap that is not kept) goes back
        # to the dump's state, or Scr_Init's dvar registration reports hash collisions.
        tables = builtin_table_ranges(self.img.timestamp)
        regions = [(self.B + rva, bytes(self.uc.mem_read(self.B + rva, n))) for rva, n in tables]
        data = [s for s in self.img.sections if s[0] == ".data"][0]
        step = 0x1000000
        restored = 0
        for a in range(data[1], data[1] + data[2], step):
            n = min(step, data[1] + data[2] - a)
            now = bytes(self.uc.mem_read(self.B + a, n))
            old = self.img.d[a:a + n]
            if now == old:
                continue
            for p in range(0, n, 0x1000):
                if now[p:p + 0x1000] != old[p:p + 0x1000]:
                    self.uc.mem_write(self.B + a + p, old[p:p + 0x1000])
                    restored += 1
        self.emu.heap_next, self.big_next = heap0, big0
        for va, b in regions:
            self.uc.mem_write(va, b)
        blob = b"".join(struct.pack("<QI", va, len(b)) + b for va, b in regions)
        os.makedirs(os.path.dirname(cache), exist_ok=True)
        with open(cache, "wb") as f:
            f.write(zlib.compress(blob, 6))
        filled = sum(1 for _, b in regions for k in range(0, len(b), 0x20)
                     if struct.unpack_from("<I", b, k)[0])
        self.boot_log.append("static init: %d initialisers, %d ran to their return; %d builtin "
                             "tables kept (%d entries non-zero), %d other pages restored"
                             % (len(funcs), ok, len(regions), filled, restored))

    def add_script(self, name, data):
        """Make `name` (e.g. scripts/zz/x.gsc) findable by the asset lookup."""
        buf = self.big_alloc(len(data) + 16, 16)
        self.uc.mem_write(buf, data)
        spt = self.big_alloc(0x18)
        self.w64(spt, self.put_cstr(name))
        self.w32(spt + 8, len(data))
        self.w64(spt + 0x10, buf)
        self.assets[name.lower()] = spt
        self.script_buffers[name.lower()] = (buf, len(data))  # loaded in place (the tracer's map)
        return spt

    def _find_asset(self):
        t, name = self.emu.arg(0) & 0xFFFFFFFF, self.cstr(self.emu.arg(1))
        self.asset_requests.append((t, name))
        if t == ASSET_SCRIPTPARSETREE and name and name.lower() in self.assets:
            return self.assets[name.lower()]
        return 0

    def load(self, name):
        """Scr_BeginLoadScripts, Scr_LoadScript(name without extension), Scr_EndLoadScripts:
        returns Scr_LoadScript's result (1 loaded and linked, 0 not)."""
        inst = self.inst
        self.call(ADDR["Scr_BeginLoadScripts"], inst, 1)
        r = self.call(ADDR["Scr_LoadScript"], inst, self.put_cstr(name))
        return r

    def end_load(self):
        self.call(ADDR["Scr_EndLoadScripts"], self.inst)

    def function_handle(self, script, namespace, func):
        """Scr_GetFunctionHandle(inst, script, ns canon, func canon, 0, 1): code address or 0."""
        return self.call(ADDR["Scr_GetFunctionHandle"], self.inst, self.put_cstr(script),
                         t7_canon(namespace), t7_canon(func), 0, 1)

    # --- running -----------------------------------------------------------------------------
    def exec_thread(self, handle, self_id=0):
        """Scr_ExecThread(inst, codepos, 0 params, no return slot, self): the thread runs at once
        until its first wait or its end; returns the thread's variable id."""
        return self.call(ADDR["Scr_ExecThread"], self.inst, handle, 0, 0, self_id,
                         count=self.step_count)

    def run_frame(self):
        """One server frame as G_RunFrame (0x19e1c30) drives the VM: Scr_RunCurrentThreads in
        its entity loop (0x19e914b), then at its end Scr_RunFrame (0x19e939b: the threads due
        once more, then gScrVarPub.time += 1). With no entities the loop runs once."""
        self.frame += 1
        if self.frame_model == "g_runframe":
            self.call(ADDR["Scr_RunCurrentThreads"], self.inst, count=self.step_count)
        self.call(ADDR["Scr_RunFrame"], self.inst, count=self.step_count)

    def now(self):
        """gScrVarPub.time: the VM's frame counter, the trace's time stamp (one server frame,
        50 ms, per unit: wait 0.05 resumes at time + 1)."""
        return self.pub(getattr(self, "inst", 0), "time")

    def current_thread(self):
        """The variable id of the running thread. function_state.localId is the current
        function's local frame: a script call allocates a child_thread variable whose parent id
        (+0x38, measured) is the caller's; the thread is the root of that chain (type thread)."""
        v = self.r32(self.B + VM_FS + 0x20 * self.inst + 0x10)
        for _ in range(256):
            _, t, raw = self.var(self.inst, v)
            if t != VT["child_thread"]:
                return v
            v = struct.unpack_from("<I", raw, 0x38)[0]
        return v

    def value_text(self, va):
        """A VM value {u64, u32 type} as text, for the trace."""
        v, t = struct.unpack("<QI", bytes(self.uc.mem_read(va, 12)))
        if t == VT["undefined"]:
            return "undefined"
        if t in (VT["string"], VT["istring"]):
            return self.sl_string(v & 0xFFFFFFFF)
        if t == VT["int"]:
            return str(struct.unpack("<q", struct.pack("<Q", v))[0])
        if t == VT["float"]:
            f = struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]
            return ("%g" % f)
        if t == VT["hash"]:
            return "#%x" % (v & 0xFFFFFFFF)
        if t == VT["vector"]:
            x, y, z = struct.unpack("<fff", bytes(self.uc.mem_read(v, 12)))
            return "(%g, %g, %g)" % (x, y, z)
        return "<%s %#x>" % (VT_NAMES.get(t, "type%d" % t), v)

    def sl_string(self, idx):
        """SL_ConvertToString: the memory-tree node pool + idx * 0x1c + 4."""
        if not idx:
            return ""
        return self.cstr(self.r64(self.B + SL_POOL) + 0x1C * idx + 4, 4096)

    def builtin_args(self):
        """The arguments of the builtin being called: scrVmPub.outparamcount values below top."""
        pub = self.B + GVM_PUB + 0x8A40 * self.inst
        n = self.r32(pub + 0x38)
        top = self.r64(pub + 0x20)
        return [self.value_text(top - 0x10 * i) for i in range(n)]

    def track_threads(self):
        """Name threads T0, T1... in the order they are created: watch the return of
        ScrVar_AllocThread (0x12d9960) at its call sites in the thread-call handlers and the
        Scr_Exec*Thread APIs (not the Endon handler's). A reused id gets the next name."""
        def on(vm):
            tid = vm.uc.reg_read(X.UC_X86_REG_EAX)
            vm.thread_names[tid] = "T%d" % vm.thread_count
            vm.thread_count += 1
            for cb in vm.on_thread_alloc:
                cb(tid, vm.thread_names[tid])
        for site in ALLOC_THREAD_SITES:
            self.watch(site + 5, on)

    def thread_name(self, tid):
        return self.thread_names.get(tid, "thread%d" % tid)

    def trace_errors(self):
        """Record each script error where the VM throws it (0x12ea4e0, reached from Scr_Error
        and from the invalid-opcode path), with gScrVarPub.error_message and the terminal flag;
        the game's own handling then runs."""
        def on(vm):
            msg = vm.cstr(vm.r64(vm.B + GVAR_PUB + 0x78 * vm.inst + 0x10), 1024) or ""
            term = vm.uc.mem_read(vm.B + GVM_PUB + 0x8A40 * vm.inst + 0x2B, 1)[0]
            vm.errors.append((vm.now(), vm.thread_name(vm.current_thread()), msg, bool(term),
                              len(vm.trace)))
            for cb in vm.on_error:
                cb(msg, bool(term))
            if len(vm.errors) >= ERROR_FLOOD:
                vm.stop_reason = "error flood: %d script errors (last: %s)" % (len(vm.errors), msg)
                vm.uc.emu_stop()
        self.watch(ADDR["Scr_Throw"], on)

    def trace_builtin(self, rva, label):
        """Record (frame, thread, label, args) each time the builtin handler at rva is called;
        the handler itself is not run."""
        def on(vm):
            vm.trace.append((vm.now(), vm.thread_name(vm.current_thread()), label,
                             vm.builtin_args()))
            return 0
        self.hook(rva, on, "builtin " + label, "recorded in the trace, handler not run")

    # --- globals ---------------------------------------------------------------------------
    def pub(self, inst, field):
        return self.r32(self.B + GVAR_PUB + 0x78 * inst + PUB_FIELDS[field])

    def var_base(self, inst):
        return self.r64(self.B + GVAR_GLOB + 0x100 * inst + 0x80)

    def var(self, inst, idx):
        """One 0x40-byte variable: (value u64, type u32, raw)."""
        a = self.var_base(inst) + VAR_SIZE * idx
        raw = bytes(self.uc.mem_read(a, VAR_SIZE))
        v, t = struct.unpack_from("<QI", raw)
        return v, t, raw
