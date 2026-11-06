"""The Call of Duty 4 (IW3) script VM run offline under Unicorn, from the Steam multiplayer
executable (iw3mp.exe 1.8, file layout): the same harness as World at War (t4vm.OldVM), with the
IW3 addresses. IW3's VM has one instance (no inst arguments): the calls below differ from T4's
only in their register and stack arguments.

Research tool only (Unicorn, GPL-2.0-only, from Python); writes data files.

Addresses are VAs of the executable named by `exe.iw3` (PE timestamp 1525213173, image base
0x400000). Each is pinned by what calls it and what it does when run (reports/vm_iw3.md).
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from t4vm import OldVM  # noqa: E402
from vmcore32 import Fault  # noqa: E402,F401

IW3 = {
    "game": "iw3",
    "timestamp": 1525213173,
    # static CRT: _cfltcvt_init (the table at 0x713f88 starts as _fptrap 0x67d176)
    "crt_init": (0x6730CC,),
    # the same CRT as T4's, at other addresses: _getptd_noexit (0x66f58a) replaced by one block
    # filled as _initptd (0x66f4cb) fills it; _osplatform/_winmajor (0x7223a8, 0x7223b4) set
    # as the start-up sets them; _lock (0x671593, lock table 0x713b28) returns at once
    "_getptd_noexit": 0x66F58A,
    "ptd_init": {0x5C: 0x713FB0, 0x14: 1, 0x70: 1, 0x68: 0x713500, 0x04: 0xFFFFFFFF,
                 0x6C: ("deref", 0x713B08)},
    "ptd_bytes": {0xC8: 0x43, 0x14B: 0x43},
    "crt_globals": {0x7223A8: 2, 0x7223B4: 6},
    "noop": {0x671593: "_lock"},
    # Sys_InitMainThread (TLS +0x14 -> 0x14e0920, the va() buffers 0xcc06e78)
    "engine_init": (0x505D70,),
    # Com_InitHunkMemory (Com_Init 0x4f9e0f); Hunk_InitDebugMemory is inlined in Com_Init
    # (0x4f9e14-0x4f9e81: a 16 MiB VirtualAlloc user stored at 0xcb1a4e4): replicated
    "Com_InitHunkMemory": 0x55E060,
    "debug_hunk": {"user": 0xCB1A4E4, "size": 0x1000000, "name": 0x6EA17C},
    # SL_Init() (Com_Init 0x4f9c44; MT_Init 0x5116f0 then the 20000-entry string table)
    "SL_Init": 0x512A20,
    # InitVariableRange(begin in edi, end in eax): Com_Init 0x4fa198 / 0x4fa1a7
    "InitVariableRange": 0x513C70,
    "var_ranges": ((1, 0x8001), (0x8002, 0x18000)),
    # Scr_Init() (Com_Init 0x4fa1ac)
    "Scr_Init": 0x517C90,
    # Scr_InitSystem() (G_InitGame 0x4ba69f): time 0x15c2624, timeArrayId 0x15c2628,
    # pauseArrayId 0x15c262c, levelId 0x15c2630, gameId 0x15c2634, animId 0x15c2638
    "Scr_InitSystem": 0x51D850,
    # Scr_BeginLoadScripts() (GScr_LoadScripts 0x4c5bd8), Scr_LoadScript(name in ecx),
    # Scr_GetFunctionHandle(file, name) cdecl (0x4c5c09), Scr_EndLoadScripts() (push 2 + the
    # compile-state objects freed, as T4's 0x689c80)
    "Scr_BeginLoadScripts": 0x510A60,
    "Scr_LoadScript": 0x510E60,
    "Scr_GetFunctionHandle": 0x510880,
    "Scr_EndLoadScripts": 0x510E80,
    # Scr_ExecThread(handle in eax) -> thread id (G_InitGame 0x4ba771), then the id freed
    # (0x514a40, cdecl)
    "Scr_ExecThread": 0x51D560,
    "Scr_FreeThread": 0x514A40,
    # G_RunFrame (0x4bb3f0) runs Scr_RunCurrentThreads' body inline twice (0x4bb6a1, 0x4bb7c6:
    # the time array entry of scrVarPub.time, VM_Resume 0x51d1c0), then 0x516820 and
    # time += 1 (0x4bb87c, time &= 0xffffff); 0x51d7e0 is that body as a function
    "Scr_RunCurrentThreads": 0x51D7E0,
    "frame_tail": 0x516820,
    "VM_Execute": 0x51D360,
    "VM_ExecuteInternal": 0x517E50,   # _setjmp3 0x684904, jmp_buf 0x178b800 + depth*64,
                                      # opcode table 0x51b7dc (0..0x86)
    # Scr_ErrorInternal(): longjmp (0x66b0dc) into the VM when a thread runs, else
    # Sys_Error("%s", error_message)
    "Scr_ErrorInternal": 0x517CF0,
    "longjmp": 0x66B0DC,
    "error_message": 0x15C261C,
    "terminal_error": 0x178C06E,
    # OP_jumpback (0x63, handler 0x51a3d9): the 2500 ms guard, as T4's
    "loop_kill": (0x51A49E, 0x6DD320),
    "vsnprintf": 0x669185,
    "Com_PrintMessage": 0x4F75E0,   # (channel, msg) under Com_Printf 0x4f7740
    "Com_Error": 0x4F7ED0,          # (code, fmt, ...)
    "Sys_Error": 0x571140,          # (fmt, ...)
    "FS_FOpenFileRead": 0x556020,   # (name, &handle, 0) -> length
    "FS_Read": 0x5567E0,
    "FS_FCloseFile": 0x555A70,      # (handle in eax)
    # data
    "vars": 0x160B810, "vars_per_inst": 0,
    "time": 0x15C2624, "top": 0x178C068, "outparamcount": 0x178C074, "child_type": 17,
    "levelId": 0x15C2630,
    "typenames": 0x71C980,
    "sl_buffer": 0x14E0A04,
    "fs": 0x178C040,                # +0 pos, +4 localId
    # Scr_Settings (0x4f9580): developer 0x15c2616, developer_script 0x15c2617,
    # abort_on_error 0x178c06d (= the developer dvar)
    "settings": {"developer": (0x15C2616,), "developer_script": (0x15C2617,),
                 "abort_on_error": (0x178C06D,)},
    # thread variables typed 0x6e (thread): opcodes 0x54-0x57 and the Exec APIs
    "thread_alloc_sites": (0x519D60, 0x519E5F, 0x519F70, 0x51A024, 0x51D5D2, 0x51D6BD,
                           0x51D7A2),
    # builtins/iw3.toml: logprint (MP) and the dev-only println
    "log_builtin": ("logprint", 0x4CA1B0),
    "extra_trace": {"println": 0x4C60F0},
}


class IW3VM(OldVM):
    def boot(self, developer=False):
        P = self.P
        for va in P.get("crt_init", ()):
            self.call(va)
            self.boot_log.append("CRT init %#x" % va)
        for va in P.get("engine_init", ()):
            self.call(va)
            self.boot_log.append("engine init %#x" % va)
        self.call(P["Com_InitHunkMemory"])
        self._debug_hunk()
        self.call(P["SL_Init"])
        for lo, hi in P["var_ranges"]:
            self.call(P["InitVariableRange"], edi=lo, eax=hi)
        self.call(P["Scr_Init"])
        self.settings(developer)
        self.call(P["Scr_InitSystem"])
        self.boot_log.append("level %d time %d" % (self.r32(P["levelId"]), self.now()))

    def _debug_hunk(self):
        """Com_Init's inlined Hunk_InitDebugMemory (0x4f9e14-0x4f9e81), replicated: a 16 MiB
        hunk user {self, ?, size, end, next = base + 0x20, name, 0, 0, 0}."""
        d = self.P["debug_hunk"]
        base = self.big_alloc(d["size"], 0x1000)
        self.w32(base, base)
        self.w32(base + 8, d["size"])
        self.w32(base + 0xC, base + d["size"])
        self.w32(base + 0x10, base + 0x20)
        self.w32(base + 0x14, d["name"])
        self.w32(d["user"], base)
        self.note("Hunk_InitDebugMemory (inlined in Com_Init)", "replicated")

    def load(self, name):
        self.call(self.P["Scr_BeginLoadScripts"])
        return self.call(self.P["Scr_LoadScript"], ecx=self.put_cstr(name))

    def function_handle(self, script, func):
        return self.call(self.P["Scr_GetFunctionHandle"], self.put_cstr(script),
                         self.put_cstr(func))

    def end_load(self):
        self.call(self.P["Scr_EndLoadScripts"])

    def exec_thread(self, handle):
        tid = self.call(self.P["Scr_ExecThread"], eax=handle, count=self.step_count) & 0xFFFF
        self.call(self.P["Scr_FreeThread"], tid)
        return tid

    def run_frame(self):
        """G_RunFrame with no entities: the runner twice, 0x516820, time += 1 (the two
        instructions of 0x4bb87c / 0x4bb888 replicated)."""
        P = self.P
        self.frame += 1
        self.clock_ms += self.clock_frame_ms
        self.call(P["Scr_RunCurrentThreads"], count=self.step_count)
        self.call(P["Scr_RunCurrentThreads"], count=self.step_count)
        self.call(P["frame_tail"], count=self.step_count)
        self.w32(P["time"], (self.r32(P["time"]) + 1) & 0xFFFFFF)
