"""The World at War (T4) script VM run offline under Unicorn, from the unwrapped SP executable:
the game's own init (SL, variables, Scr_Init, Scr_InitSystem), its own compiler (the plain-text
.gsc is parsed and compiled at load by Scr_LoadScript), its scheduler and opcode handlers. Only
what the VM needs from the rest of the engine is stubbed, and every stub is logged.

This is a research tool, not part of the shipped toolchain: it imports Unicorn (GPL-2.0-only)
from Python, like the rest of tools/re, and writes data files (traces, facts).

Addresses are VAs of the image named by `exe.t4` (codwaw_flat.exe, PE timestamp 1445666630,
image base 0x400000). Every name is a hypothesis checked by running and by the code that calls
it (the reading is in reports/vm_t4.md); names follow the Quake 3 / CoD4 lineage only as labels.
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from vmcore32 import GameVM32, Fault, X  # noqa: E402,F401

# --- profile: every address with the code that pins it --------------------------------------
T4 = {
    "game": "t4",
    "timestamp": 1445666630,
    # SL_Init(inst in eax): MT_Init (0x68a4f0) then the 25000-entry string hash table under a
    # critical section; Com_Init calls it for inst 0 and 1 (0x59cf16, 0x59cf22)
    "SL_Init": 0x68D9F0,
    # Com_InitHunkMemory(): Com_Init 0x59d0f3; reserves the hunk (VirtualAlloc 256 MiB with
    # the null useFastFile dvar read as 0) that Hunk_AllocateTempMemoryHigh (0x5e4220) and the
    # compiler's allocations use
    "Com_InitHunkMemory": 0x5E3CA0,
    # Hunk_InitDebugMemory(): Com_Init's next call (0x59d0f8); the debug hunk user
    # (0x212b2ec) that developer mode's script source and debugger tables allocate from
    "Hunk_InitDebugMemory": 0x5E4650,
    # InitVariableRange(begin in edi, end in esi, inst on the stack): Com_Init (0x59d46f,
    # 0x59d483) calls it for [1, 0x5fff) and [0x6000, 0x15ffe) of inst 0: the free lists of the
    # 0x16000 variables at 0x3914710 (stride 0x10)
    "InitVariableRange": 0x68EF90,
    "var_ranges": ((1, 0x5FFF), (0x6000, 0x15FFE)),
    # Scr_Init(inst in eax): Com_Init 0x59d48d right after; registers sv_clientside, sets
    # scrVarPub.bInited (0x3882ba4) and clears the compiler's state
    "Scr_Init": 0x693C20,
    # Scr_InitSystem(inst in edi): timeArrayId, pauseArrayId, levelId, animId, time = 0;
    # G_LoadLevel (0x503300) calls it, then allocates gameId (0x68fe20 + 0x692d70) when absent
    "Scr_InitSystem": 0x699860,
    "AllocObject": 0x68FE20,     # (inst in eax) -> id
    "gameId_init": 0x692D70,     # (id) cdecl, after G_LoadLevel's AllocObject of gameId
    # Scr_BeginLoadScripts(inst in edi, user on the stack): GScr_LoadScripts 0x51487e; sets up
    # its own hunk (VirtualAlloc, "Scr_BeginLoadScripts") and the compile-state objects
    "Scr_BeginLoadScripts": 0x689660,
    # Scr_LoadScript(filename in ecx, inst in edx): GScr_LoadScripts ("maps/_callbacksetup",
    # 0x514777); calls Scr_LoadScriptInternal (0x689980): "%s.gsc", the source reader, the
    # parser (0x69aec0, "bad syntax") and the compiler
    "Scr_LoadScript": 0x689C60,
    # Scr_GetFunctionHandle(filename in eax, inst in ecx, function name on the stack) -> code
    # offset (0x51479c after Scr_LoadScript)
    "Scr_GetFunctionHandle": 0x6894B0,
    # Scr_EndLoadScripts(inst) cdecl (0x51528e)
    "Scr_EndLoadScripts": 0x689C80,
    # Scr_ExecThread(inst in edi, handle, paramcount) -> thread id (ax): AllocThread(level),
    # VM_Execute (0x6992e0); G_LoadLevel 0x503366 then frees the id (0x690040, eax id, ecx inst)
    "Scr_ExecThread": 0x699560,
    "Scr_FreeThread": 0x690040,
    # one server frame with no entities: G_RunFrame (0x503ab0) calls Scr_RunCurrentThreads
    # (inst in eax) at 0x503c4d, then (entity loops) at 0x503e48 followed inline by
    # Scr_FreeEntityList-like 0x692160 and time += 1 (time &= 0xffffff); 0x69ade0 (inst in eax)
    # is that same tail as a function (runner, 0x692160, time += 1)
    "Scr_RunCurrentThreads": 0x6997E0,
    "Scr_RunFrameTail": 0x69ADE0,
    # VM_Execute(inst, localId, pos, paramcount) cdecl; VM_ExecuteInternal (0x693e80): _setjmp3
    # (0x7e1894) into jmp_buf 0x3bdcd40 + (inst*33 + depth)*64, opcode table 0x697558 (0..0x86)
    "VM_Execute": 0x6992E0,
    "VM_ExecuteInternal": 0x693E80,
    # Scr_ErrorInternal(inst in eax): longjmps (0x7ad57c, value -1) to the VM when a thread is
    # running, else Com_Error("%s", scrVarPub.error_message)
    "Scr_ErrorInternal": 0x693CF0,
    "longjmp": 0x7AD57C,
    # RuntimeError(inst in eax, codepos, index, msg, dialog) prints "script runtime error"
    "RuntimeError": 0x68B790,
    # Scr_NotifyNum (events phase: 0x698cc0); VM_Notify, the core it calls
    "Scr_NotifyNum": 0x698CC0,
    # CRT and engine services
    "vsnprintf": 0x7AA87B,        # _vsnprintf(buf, count, fmt, va_list)
    "Com_PrintMessage": 0x59A170,  # (channel, msg, ...) under Com_Printf (0x59a2c0)
    "Com_Error": 0x59AC50,         # (code, fmt, ...)
    "Sys_Error": 0x5FE8C0,         # (fmt, ...)
    "FS_FOpenFileRead": 0x5DBD20,  # (name, &handle) -> length; Scr_ReadFile's file path
    "FS_Read": 0x5DBDF0,           # (buf, len, handle)
    "FS_FCloseFile": 0x5DB060,     # (handle in eax)
    # static CRT: _getptd_noexit (TlsGetValue of the CRT's index, else allocate and _initptd
    # 0x7b14b8): in a process whose CRT never ran its thread init it recurses through
    # _decode_pointer; replaced by one per-thread block built as _initptd builds it
    "_getptd_noexit": 0x7B1577,
    "ptd_init": {0x5C: 0x8CD360, 0x14: 1, 0x70: 1, 0x68: 0x8CC9A0, 0x04: 0xFFFFFFFF,
                 0x6C: ("deref", 0x8CCFA8)},
    "ptd_bytes": {0xC8: 0x43, 0x14B: 0x43},
    # the CRT start-up's OS globals (_osplatform = 2 NT, _winmajor = 6): with them at 0, the
    # pointer-encoding probe (0x7b12ef -> 0x7ac281) raises an invalid parameter, whose handler
    # decodes a pointer through the same probe, forever
    "crt_globals": {0x8E7650: 2, 0x8E765C: 6},
    # functions that return at once: the CRT's _lock(n) (its lock table was never set up by
    # _mtinitlocks; one emulated thread needs no lock; _unlock 0x7b34e4 then calls the
    # LeaveCriticalSection stub)
    "noop": {0x7B35BC: "_lock"},
    # CRT start-up functions run at boot (the CRT's own code): _cfltcvt_init (called by _fpmath
    # 0x7af5d5) fills the floating-point conversion table that sscanf("%f") uses for the
    # compiler's float literals (left at _fptrap, "floating point not loaded", otherwise)
    "crt_init": (0x7AF575,),
    # Sys_InitMainThread (Com_Init path 0x5ff668): the main thread's data, TLS +0x14 ->
    # 0x1ff50a8, whose va() buffers (0x2260ba8) the error formatter uses; without it va()
    # formats into the null page that other null-based reads share
    "engine_init": (0x5A2F90,),
    # data
    "scrVarPub": 0x3882B00,       # stride 0x18048: +0x84 time, +0x88 timeArrayId, +0x8c
                                  # pauseArrayId, +0x90 levelId, +0x94 gameId, +0x98 animId,
                                  # +0x7c error_message, +0xa4 bInited, +0xb8 programBuffer
    "scrVarPub_stride": 0x18048,
    "scrVmPub": 0x3BD4700,        # stride 0x4320: +0x8 function_count, +0xc function_frame,
                                  # +0x10 top, +0x18 inparamcount, +0x1c outparamcount
    "scrVmPub_stride": 0x4320,
    "vars": 0x3914710,            # variables (0x10 bytes): +0 value, +4 refcount u16, +6 parent?, +8 type
    "vars_per_inst": 0x16000,
    # absolute addresses the trace reads (inst 0): scrVarPub.time, scrVmPub.top and
    # outparamcount; the child-thread type (the type-name table's fourth "thread")
    "time": 0x3882B84, "top": 0x3BD4710, "outparamcount": 0x3BD471C, "child_type": 16,
    "child_vars": 0x3974700,
    "iprintln": 0x515E10,          # builtins/t4.toml: gsc function table 1 entry 4
    "log_builtin": ("iprintln", 0x515E10),
    # other builtins recorded in the trace as `<name>: <args>` (handler not run): the dev-only
    # println (builtins/t4.toml, rva 0x1155e0)
    "extra_trace": {"println": 0x5155E0},
    # the VM's own type-name table (var_typename): 0 undefined, 1 object, 2 string, 3 localized
    # string, 4 vector, 5 float, 6 int ... 13-16 threads, 17 struct, 19 entity, 20 array
    "typenames": 0x8CF4B0,
    # SL_ConvertToString: the memory tree buffer of the inst (0x3702390 + inst*4) + id*12 + 4
    "sl_buffer": 0x3702390,
    # the running function's state (inst*20 stride): +0 pos, +4 localId
    "fs": 0x3BDDDD0,
    # scrVarPub.developer (+0x76), developer_script (+0x77), scrVmPub.abort_on_error (+0x15),
    # and the inst-1 copies (0x389abbe/f, 0x3bd8a35), as Scr_Settings (0x59c840) writes them
    "settings": {"developer": (0x3882B76, 0x389ABBE), "developer_script": (0x3882B77, 0x389ABBF),
                 "abort_on_error": (0x3BD4715, 0x3BD8A35)},
    # where a new thread variable gets its type (0x6d = thread | flags): the four thread-call
    # opcodes of VM_ExecuteInternal (0x54-0x57, table 0x697558) and Scr_ExecThread /
    # ExecEntThread / AddExecThread. Not 0x696b83: opcode 0x7a (endon: object on top, event
    # string below) types its endon record the same way (first guess named endons as threads)
    "thread_alloc_sites": (0x695D34, 0x695E30, 0x695F37, 0x695FC1, 0x6995E2, 0x6996CE,
                           0x6997A6),
    # Scr_ErrorInternal's entry: scrVarPub.error_message (+0x7c, char*), terminal_error
    # (scrVmPub+0x16)
    "error_message": 0x3882B7C,
    # OP_jumpback (0x63, handler 0x6962b0): when the execution has run 2500 ms of wall clock
    # (timeGetTime, since Scr_ResetTimeout 0x69ae60) it warns and resets under the debugger,
    # kills the thread with this message when abort_on_error is clear (0x69639c), and raises
    # the terminal script error "potential infinite loop in script" otherwise (0x696486)
    "loop_kill": (0x69639C, 0x89AAB0),
    "terminal_error": 0x3BD4716,
}


class OldVM(GameVM32):
    """The Q3-lineage 32-bit script VM (T4, IW3): one profile dict of addresses."""

    def __init__(self, exe, prof, log=print, verbose=False):
        super().__init__(exe, log=log, verbose=verbose)
        self.P = P = prof
        if self.img.timestamp != P["timestamp"]:
            raise SystemExit("%s: PE timestamp %d, the profile is for %d"
                             % (exe, self.img.timestamp, P["timestamp"]))
        self.trace = []
        self.errors = []
        self.thread_names = {}
        self.thread_count = 0
        self.step_count = 20_000_000
        self.frame = 0
        self.files = {}           # path (lower case) -> bytes
        self.handles = {}
        self.file_requests = []
        self.hook(P["vsnprintf"], OldVM.vsnprintf_hook, "_vsnprintf", "Python formatter")
        self.hook(P["Com_PrintMessage"], OldVM._print, "Com_PrintMessage", "message recorded")
        self.hook(P["Com_Error"], OldVM._com_error, "Com_Error", "recorded; stops the run")
        self.hook(P["Sys_Error"], OldVM._sys_error, "Sys_Error", "recorded; stops the run")
        self.hook_longjmp(P["longjmp"])
        self.hook(P["FS_FOpenFileRead"], OldVM._fs_open, "FS_FOpenFileRead",
                  "the test scripts only (file system)")
        self.hook(P["FS_Read"], OldVM._fs_read, "FS_Read", "copies the test script")
        self.hook(P["FS_FCloseFile"], lambda vm: 0, "FS_FCloseFile", "no file system")
        for va, v in P.get("crt_globals", {}).items():
            self.w32(va, v)
            self.note("CRT global %#x = %d" % (va, v), "set as the CRT start-up sets it")
        for va, name in P.get("noop", {}).items():
            self.hook(va, lambda vm: 0, name, "returns at once (single emulated thread)")
        if P.get("_getptd_noexit"):
            self.ptd = self._make_ptd()
            self.hook(P["_getptd_noexit"], lambda vm: vm.ptd, "_getptd_noexit",
                      "one CRT per-thread block, filled as _initptd fills it")

    def _make_ptd(self):
        P = self.P
        a = self.big_alloc(0x400, 16)
        self.w32(a, 0x1000)  # _tid
        for off, v in P["ptd_init"].items():
            if isinstance(v, tuple):
                v = self.r32(v[1])
            self.w32(a + off, v)
        for off, v in P.get("ptd_bytes", {}).items():
            self.w8(a + off, v)
        return a

    # --- engine services -------------------------------------------------------------------
    def _print(self):
        ch, msg = self.arg(0), self.cstr(self.arg(1), 4096) or ""
        self.prints.append((self.now(), ch, msg))
        if self.verbose:
            self.log("[print %d] %s" % (ch, msg.rstrip()))
        return 0

    def _com_error(self):
        code = self.arg(0)
        text = self.format(self.cstr(self.arg(1), 4096) or "", lambda k: self.r32(
            self.uc.reg_read(X.UC_X86_REG_ESP) + 12 + 4 * k))
        self.prints.append((self.now(), -1, "Com_Error(%d): %s" % (code, text)))
        self.stop_reason = "Com_Error: " + text.strip()
        self.uc.emu_stop()
        return 0

    def _sys_error(self):
        text = self.format(self.cstr(self.arg(0), 4096) or "", lambda k: self.r32(
            self.uc.reg_read(X.UC_X86_REG_ESP) + 8 + 4 * k))
        self.stop_reason = "Sys_Error: " + text.strip()
        self.uc.emu_stop()
        return 0

    def _fs_open(self):
        name, ph = self.cstr(self.arg(0)) or "", self.arg(1)
        self.file_requests.append(name)
        data = self.files.get(name.lower().replace("\\", "/"))
        if data is None:
            if ph:
                self.w32(ph, 0)
            return 0xFFFFFFFF
        h = len(self.handles) + 1
        self.handles[h] = data
        if ph:
            self.w32(ph, h)
        return len(data)

    def _fs_read(self):
        buf, n, h = self.arg(0), self.arg(1), self.arg(2)
        data = self.handles.get(h, b"")[:n]
        self.uc.mem_write(buf, data)
        return len(data)

    # --- globals ---------------------------------------------------------------------------
    def pub(self, off, inst=0):
        return self.r32(self.P["scrVarPub"] + self.P["scrVarPub_stride"] * inst + off)

    def now(self):
        """scrVarPub.time: the VM's frame counter (one 50 ms server frame per unit)."""
        return self.r32(self.P["time"]) & 0xFFFFFF

    def vmpub(self, off, inst=0):
        return self.P["scrVmPub"] + self.P["scrVmPub_stride"] * inst + off

    # --- values --------------------------------------------------------------------------------
    def typename(self, t):
        if not hasattr(self, "_typenames"):
            self._typenames = []
            for k in range(32):
                p = self.r32(self.P["typenames"] + 4 * k)
                n = self.img.cstr(p, 40) if self.img.in_image(p) else None
                if not n:
                    break
                self._typenames.append(n)
        return self._typenames[t] if t < len(self._typenames) else "type%d" % t

    def sl_string(self, idx, inst=0):
        return self.cstr(self.r32(self.P["sl_buffer"] + 4 * inst) + 12 * idx + 4, 4096) or ""

    def value_text(self, va):
        """A VM value {u32, u32 type} as text, for the trace."""
        v, t = struct.unpack("<II", bytes(self.uc.mem_read(va, 8)))
        t &= 0x1F
        name = self.typename(t)
        if name == "undefined":
            return "undefined"
        if name in ("string", "localized string"):
            return self.sl_string(v)
        if name == "int":
            return str(struct.unpack("<i", struct.pack("<I", v))[0])
        if name == "float":
            return "%g" % struct.unpack("<f", struct.pack("<I", v))[0]
        if name == "vector":
            x, y, z = struct.unpack("<fff", bytes(self.uc.mem_read(v, 12)))
            return "(%g, %g, %g)" % (x, y, z)
        return "<%s %#x>" % (name, v)

    def builtin_args(self):
        """The arguments of the builtin being called: outparamcount values below top."""
        n = self.r32(self.P["outparamcount"])
        top = self.r32(self.P["top"])
        return [self.value_text(top - 8 * i) for i in range(n)]

    def var(self, idx, inst=0):
        """One 16-byte variable: (value u32, word +4, word +6, type u32 at +8, raw)."""
        a = self.P["vars"] + 16 * (self.P["vars_per_inst"] * inst + idx)
        raw = bytes(self.uc.mem_read(a, 16))
        v, w4, w6, t = struct.unpack_from("<IHHI", raw)
        return v, w4, w6, t, raw

    def local_id(self, inst=0):
        return self.r32(self.P["fs"] + 20 * inst + 4)

    def current_thread(self):
        """The running thread: the root of the localId chain. A script call runs in a child
        thread variable (type 16) whose type word carries the caller's localId above bit 8
        (measured: a call from f has type 0x970 with f's frame 9); +6 is its self."""
        v = self.local_id()
        for _ in range(256):
            t = self.var(v)[3]
            if (t & 0x1F) != self.P["child_type"]:
                return v
            v = t >> 8
        return v

    # --- trace -------------------------------------------------------------------------------
    def track_threads(self):
        """Name threads T0, T1... in creation order: at each site that types a new thread
        variable (eax = the variable)."""
        P = self.P

        def on(vm):
            a = vm.reg("eax")
            tid = (a - P["vars"]) // 16 - P["vars_per_inst"] * 0
            vm.thread_names[tid] = "T%d" % vm.thread_count
            vm.thread_count += 1
        for site in P["thread_alloc_sites"]:
            self.watch(site, on)

    def thread_name(self, tid):
        return self.thread_names.get(tid, "thread%d" % tid)

    def trace_builtin(self, va, label):
        """Record (time, thread, label, args) at each call of the builtin handler at va; the
        handler is not run."""
        def on(vm):
            vm.trace.append((vm.now(), vm.thread_name(vm.current_thread()), label,
                             vm.builtin_args()))
            return 0
        self.hook(va, on, "builtin " + label, "recorded in the trace, handler not run")

    def trace_errors(self, flood=64):
        """Record each script error at Scr_ErrorInternal's entry (message, terminal flag); the
        game's own handling then runs."""
        P = self.P

        def on(vm):
            msg = vm.cstr(vm.r32(P["error_message"]), 1024) or ""
            term = vm.r8(P["terminal_error"])
            vm.errors.append((vm.now(), vm.thread_name(vm.current_thread()), msg, bool(term),
                              len(vm.trace)))
            if len(vm.errors) >= flood:
                vm.stop_reason = "error flood: %d script errors (last: %s)" % (len(vm.errors), msg)
                vm.uc.emu_stop()
        self.watch(P["Scr_ErrorInternal"], on)
        if P.get("loop_kill"):
            site, text = P["loop_kill"]

            def kill(vm):
                msg = vm.cstr(text, 200).strip().replace("script runtime error: ", "")
                vm.errors.append((vm.now(), vm.thread_name(vm.current_thread()), msg, False,
                                  len(vm.trace)))
            self.watch(site, kill)

    # --- boot and load ---------------------------------------------------------------------
    def settings(self, developer):
        """What Scr_Settings writes for developer = developer_script = (developer mode)."""
        for name, vas in self.P["settings"].items():
            for va in vas:
                self.w8(va, 1 if developer else 0)
        self.boot_log.append("Scr_Settings: developer %d" % int(developer))

    def boot(self, developer=False):
        P = self.P
        for va in P.get("crt_init", ()):
            self.call(va)
            self.boot_log.append("CRT init %#x" % va)
        for va in P.get("engine_init", ()):
            self.call(va)
            self.boot_log.append("engine init %#x" % va)
        self.call(P["Com_InitHunkMemory"])
        if P.get("Hunk_InitDebugMemory"):
            self.call(P["Hunk_InitDebugMemory"])
        self.call(P["SL_Init"], eax=0)
        for lo, hi in P["var_ranges"]:
            self.call(P["InitVariableRange"], 0, edi=lo, esi=hi)
        self.call(P["Scr_Init"], eax=0)
        self.settings(developer)
        self.call(P["Scr_InitSystem"], edi=0)
        if not self.pub(0x94):
            gid = self.call(P["AllocObject"], eax=0) & 0xFFFF
            self.w32(P["scrVarPub"] + 0x94, gid)
            self.call(P["gameId_init"], gid)
        self.boot_log.append("level %d game %d time %d" % (self.pub(0x90), self.pub(0x94),
                                                           self.now()))

    def add_script(self, path, data):
        self.files[path.lower()] = data

    def load(self, name):
        P = self.P
        self.call(P["Scr_BeginLoadScripts"], 1, edi=0)
        return self.call(P["Scr_LoadScript"], ecx=self.put_cstr(name), edx=0)

    def function_handle(self, script, func):
        return self.call(self.P["Scr_GetFunctionHandle"], self.put_cstr(func),
                         eax=self.put_cstr(script), ecx=0)

    def end_load(self):
        self.call(self.P["Scr_EndLoadScripts"], 0)

    def exec_thread(self, handle):
        tid = self.call(self.P["Scr_ExecThread"], handle, 0, edi=0, count=self.step_count) & 0xFFFF
        self.call(self.P["Scr_FreeThread"], eax=tid, ecx=0)
        return tid

    def run_frame(self):
        self.frame += 1
        self.clock_ms += self.clock_frame_ms
        self.call(self.P["Scr_RunCurrentThreads"], eax=0, count=self.step_count)
        self.call(self.P["Scr_RunFrameTail"], eax=0, count=self.step_count)
