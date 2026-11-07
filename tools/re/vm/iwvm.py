"""The Infinity Ward / Sledgehammer x64 script VMs (Modern Warfare 3 onwards) run offline under
Unicorn: the game's own string table, variable system, VM init, loader and linker
(`Scr_LoadScript`, the ScriptFile asset lookup hooked), `Scr_GetFunctionHandle`,
`Scr_ExecThread`, the frame functions and every opcode handler. Only what the VM needs from the
rest of the engine is stubbed, and every stub is logged.

The scripts are compiled by gsc-tool (the games ship no compiler: their ScriptFile assets hold
bytecode compiled offline), so the game's own loader links gsc-tool's output.

Addresses are RVAs of the executables named by `exe.<game>` (PE timestamp in PROFILES); every
name is a hypothesis checked by running, see reports/vm_<game>.md. These builds keep no runtime
error text: a script error is recorded with the opcode that raised it.

Research tool only (Unicorn is GPL-2.0-only, imported from Python like the rest of tools/re).
"""
import os
import re
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
from common.emu import X  # noqa: E402
from vmcore import GameVM, Fault, STUBS  # noqa: E402,F401

# value types (IW4's type-name table, 0x572720 in iw4mp.exe; the later builds keep no table and
# use the same numbering up to 'thread')
TYPES = ("undefined object string istring vector float int codepos precodepos function "
         "builtin_function builtin_method stack animation developer_codepos pre_animation "
         "thread notify_thread time_thread child_thread struct removed_entity entity array "
         "removed_thread").split()

# gsc-tool's opcode table of each game (file under `gsctool_src`), to name an erring opcode
CODE_TABLE = {"iw5": "iw5_pc_code.cpp", "iw6": "iw6_pc_code.cpp", "s1": "s1_pc_code.cpp",
              "h1": "h1_code.cpp", "iw7": "iw7_code.cpp"}

PROFILES = {
    "iw5": {
        "title": "Modern Warfare 3",
        "timestamp": 1786002014,
        # SL_Init (Com_Init 0x2713c1): the string table, 0x8000 entries of 12 bytes
        "SL_Init": 0x29A160,
        # Com_Init's order: SL_Init, ..., Hunk_InitMemory (0x31ca30: a 10 MiB reservation),
        # Scr_InitHunkUsers (0x296890: scrVarPub.programHunkUser, canonicalStringHunkUser),
        # ..., Scr_InitVariables, Scr_Init
        "boot": ("SL_Init", 0x31CA30, 0x296890, "Scr_InitVariables", "Scr_Init"),
        # the variable and object pools (Com_Init 0x2716f5..., then Scr_Init)
        "Scr_InitVariables": 0x29F030,
        # Scr_Init: scrVmPub (stack, function frames), the time-array of threads to free
        "Scr_Init": 0x2A1730,
        # Scr_InitSystem(): timeArrayId, pauseArrayId, ..., levelId (G_InitGame 0x19a9e0, after
        # GScr_LoadScripts); then VM_SetTime's loop once (0x2a1570)
        "Scr_InitSystem": 0x2A1810,
        # GScr_LoadScripts (0x1a4210): Scr_BeginLoadScripts, Scr_LoadScript(name),
        # Scr_GetFunctionHandle(name, function name id), Scr_EndLoadScripts
        "Scr_BeginLoadScripts": 0x296500,
        "Scr_LoadScript": 0x296940,
        "Scr_GetFunctionHandle": 0x2967D0,
        "Scr_EndLoadScripts": 0x296630,
        # the loader reads the ScriptFile asset: DB_FindXAssetHeader(type, name),
        # DB_IsXAssetDefault(type, name), and inflates its compressed stack with 0x115e10
        "DB_FindXAssetHeader": 0x115780,
        "DB_IsXAssetDefault": 0x116240,
        "inflate": 0x115E10,
        "asset_scriptfile": 0x27,
        # Scr_ExecThread(handle, self, paramcount) (0x2a0770 passes levelId); returns the thread
        # id; Scr_FreeThread(id) as GScr_LoadScripts' callers do (0x29c1b0, RemoveRefToObject)
        "Scr_ExecThread": 0x2A0780,
        "Scr_FreeThread": 0x29C1B0,
        # G_RunFrame (0x19c160) with no entities: Scr_RunCurrentThreads (0x2a1a20) twice, then
        # VM_SetTime (0x2a1570: the threads waiting on this time, then time + 1) and 0x2a6ed0
        "Scr_RunCurrentThreads": 0x2A1A20,
        "VM_SetTime": 0x2A1570,
        "frame_tail": 0x2A6ED0,
        "VM_Execute": 0x2A2620,
        "VM_ExecuteInternal": 0x2A2800,
        # VM_ExecuteInternal's setjmp returning non-zero: every script error lands here
        "error_landing": 0x2A519F,
        # OP_jumpback (0x4e): an execution that has run 5000 ms of wall clock (Sys_Milliseconds,
        # since VM_Execute's start) is killed here when the script debugger is off (0x31da060):
        # every frame of the thread unwound, no error raised (no text in this build)
        "loop_kill": 0x2A4734,
        "longjmp": 0x441BD0,         # the CRT restore (rdsspq + _JUMP_BUFFER)
        "Com_Error": 0x26FAB0,       # (code, fmt, ...)
        # AllocThread(self) (thread-call opcodes, Scr_ExecThread) and the childthread variant:
        # their `ret`s, eax = the new thread
        "thread_rets": (0x29AD8B, 0x29AB85),
        # ... except where AllocThread makes an endon's notify object, not a thread: OP_endon
        # (0x52, return 0x2a4d9a) and the endons a childthread copies (0x2a2540, 0x2a256f)
        "not_threads": (0x2A4D9A, 0x2A256F),
        # data
        "programBuffer": 0x2EC7970,
        # Scr_BeginLoadScripts takes programBuffer from 0x32a2b0: the 2 MiB block whose base
        # is at 0x6bffdf0 (where the fastfile loader places script bytecode); Scr_GetFunctionHandle
        # only returns a code address inside it: the test bytecode is placed there
        "program_mem": (0x6BFFDF0, 0x200000),
        "time": 0x2EC791C,
        "levelId": 0x2EC7930,
        "top": 0x31CF9C0, "outparamcount": 0x31CF9CC, "function_count": 0x31CF9B0,
        "localId": 0x31DC090, "opcode": 0x31DC0C0, "codepos": 0x31DC088,
        "sl_buffer": 0x2DC5588,     # pointer to 12-byte entries, the text at +4
        "obj_info": 0x2EEB980,      # 8 bytes per object: u16, u16 self, u32 parent << 8 | type
        "child_type": 0x12,
        # builtin function table: handler of id i at table + 8 * i (the registrar 0x296180;
        # functions, registered by BeginLoad's 0x1a4650)
        "builtin_table": 0x2DC2EA8,
        "log_builtin": ("logprint", 319),
    },
    "iw6": {
        "title": "Ghosts",
        "timestamp": 1786624981,
        # a dump with the dynamic CRT (VCRUNTIME140, the UCRT): its imports done in Python
        "crt": True,
        # Com_Init (0x3fe830): SL_Init (0x4178a0), ..., the hunk (0x46ab90), Scr_InitHunkUsers
        # (0x4145f0), ..., Scr_InitVariables (0x41c950), Scr_Init (0x41f210)
        "boot": (0x4178A0, 0x46AB90, 0x4145F0, 0x41C950, 0x41F210),
        "Scr_InitSystem": 0x41F2F0,
        # GScr_LoadScripts (SP: 0x36cb00): Scr_BeginLoadScripts, Scr_LoadScript,
        # Scr_GetFunctionHandle(name, id); no Scr_EndLoadScripts: its body is inlined in
        # Scr_FreeScripts (0x414410's first block), replicated here
        "Scr_BeginLoadScripts": 0x414250,
        "Scr_LoadScript": 0x4146A0,
        "Scr_GetFunctionHandle": 0x414520,
        "end_load": {"calls": ((0x417270, ()), (0x4179B0, (2,))), "script_loading": 0x4FC8D7A},
        # the loader (0x41d1a0): DB_FindXAssetHeader(0x2d, name), DB_IsXAssetDefault, inflate
        "DB_FindXAssetHeader": 0x2B0D70,
        "DB_IsXAssetDefault": 0x2B1E60,
        "inflate": 0x2B1760,
        "asset_scriptfile": 0x2D,
        "program_mem": (0x59C4240, 0x200000),
        "Scr_ExecThread": 0x41E0D0,
        "Scr_FreeThread": 0x419960,
        # G_RunFrame (SP 0x3566e0): Scr_RunCurrentThreads (0x41f510) several times, then
        # VM_SetTime (0x41f050)
        "Scr_RunCurrentThreads": 0x41F510,
        "VM_SetTime": 0x41F050,
        "VM_Execute": 0x420110,
        "VM_ExecuteInternal": 0x4202F0,
        "error_landing": 0x422C8F,   # _setjmp (import thunk 0x64ade2) returning non-zero
        "loop_kill": 0x422224,       # OP_jumpback's 5000 ms guard (0x4221cc), debugger off
        "longjmp": None,             # the import (pycrt)
        "Com_Error": 0x3FD0E0,
        # AllocThread (0x418520) and the childthread allocation (0x418290)
        "thread_rets": (0x41854B, 0x4183ED),
        "not_threads": (0x42288A, 0x42005F),   # OP_endon, the childthread's endon copies
        "programBuffer": 0x57385F0,
        "time": 0x573859C,
        "levelId": 0x57385B0,
        "top": 0x575C630, "outparamcount": 0x575C63C, "function_count": 0x575C620,
        "localId": 0x5768D90, "opcode": 0x5768DC0, "codepos": 0x5768D88,
        "sl_buffer": 0x4FCC200, "sl_stride": 16,
        "obj_info": 0x5454580,
        "child_type": 0x12,
        "builtin_table": 0x4FC8D78,
        "log_builtin": ("logprint", 0x155),
    },
    "s1": {
        "title": "Advanced Warfare",
        "timestamp": 1563218830,
        # Com_Init (0x3cff20): SL_Init (0x3f15e0), ..., the hunk (0x4b67e0: a 16 MiB
        # reservation), ..., Scr_InitHunkUsers (0x3ee1a0), ..., Scr_InitVariables (0x3f6af0: four
        # object pools of 64000), Scr_Init (0x3f9050: 0x3f6a40, then the VM 0x3f99e0)
        "boot": (0x3F15E0, 0x4B67E0, 0x3EE1A0, 0x3F6AF0, 0x3F9050),
        # the VM init registers a dvar (Dvar_RegisterBool 0x4c0be0, "loading_sre_fatal"): the
        # dvar system is not set up here, so it returns a zeroed dvar
        "fake_dvars": (0x4C0BE0,),
        # Sys_Milliseconds (0x4d8730) first runs Sys_InitTime (the CRT's time zone set-up,
        # absent in a dump): replaced by the harness's virtual clock
        "Sys_Milliseconds": 0x4D8730,
        # the static CRT's _getptd_noexit (0x70ae84) reads its per-thread block through FLS
        # function pointers resolved at run time (absent in a dump): one zeroed block instead
        "_getptd_noexit": 0x70AE84,
        "Scr_InitSystem": 0x3F9170,
        # GScr_LoadScripts (0x3309e0): Scr_BeginLoadScripts (0x3ede60), Scr_LoadScript,
        # Scr_GetFunctionHandle(name, id), ..., Scr_EndLoadScripts (0x3edf90)
        "Scr_BeginLoadScripts": 0x3EDE60,
        "Scr_LoadScript": 0x3EE250,
        "Scr_GetFunctionHandle": 0x3EE0D0,
        "Scr_EndLoadScripts": 0x3EDF90,
        # the loader (0x3f7300): DB_FindXAssetHeader(0x32, name), DB_IsXAssetDefault, inflate
        "DB_FindXAssetHeader": 0x26F0F0,
        "DB_IsXAssetDefault": 0x270320,
        "inflate": 0x26FB90,
        "asset_scriptfile": 0x32,
        "program_mem": (0xB50A590, 0x200000),
        "Scr_ExecThread": 0x3F8130,
        "Scr_FreeThread": 0x3F3940,
        # G_RunFrame (0x2f83a0): Scr_RunCurrentThreads (0x3f9400) before and after the entity
        # loop, then VM_SetTime (0x3f8f70: the threads waiting on this time, time + 1)
        "Scr_RunCurrentThreads": 0x3F9400,
        "VM_SetTime": 0x3F8F70,
        "VM_Execute": 0x3F9E40,
        "VM_ExecuteInternal": 0x3FA070,
        "error_landing": 0x3FCA8E,   # setjmp (0x6fe070) returning non-zero
        "loop_kill": 0x3FC0B6,       # OP_jumpback's 5000 ms guard (0x3fc042), debugger off
        "longjmp": 0x6FD930,
        "Com_Error": 0x3CE480,
        # AllocThread (0x3f2270) and the childthread allocation (0x3f1f60); OP_endon and the
        # childthread's endon copies both go through 0x3f9d60, whose AllocThread returns to
        # 0x3f9d8f: an endon record, not a thread
        "thread_rets": (0x3F229B, 0x3F20BE),
        "not_threads": (0x3F9D8F,),
        "programBuffer": 0x815DEF8,
        "time": 0x815DE9C,
        "levelId": 0x815DEB0,
        "top": 0x87FC1E0, "outparamcount": 0x87FC1EC, "function_count": 0x87FC1D0,
        "localId": 0x8806748, "opcode": 0x8806768, "codepos": 0x8806740,
        "sl_buffer": 0x7DD5B08, "sl_stride": 12,
        "obj_info": 0x8185F80,
        "child_type": 0x12,
        "builtin_table": 0x7DD1848,
        "log_builtin": ("logprint", 0x160),
    },
    "h1": {
        "title": "Modern Warfare Remastered",
        "timestamp": 1563905520,
        # Com_Init (0x15c420): SL_Init (0x5085b0) and the hunk's set-up (0x59ddf0) are encrypted
        # in the dump (invalid code at rest): not run; the dump's data already holds what they
        # set (the dump was taken after start-up); then Scr_InitHunkUsers (0x504ef0),
        # Scr_InitVariables (0x50db30), Scr_Init (0x5100b0)
        "boot": (0x504EF0, 0x50DB30, 0x5100B0),
        # the script-string memory tree is the running game's (full): MT_Init replicated at this
        # build's addresses (MT_AllocIndex 0x505200: bitmap 0xb087d80, masks 0xb08fd80, search
        # starts 0xb08fd94)
        "mt_reset": {"bitmap": 0xB087D80, "words": 0x2000, "masks": 0xB08FD80,
                     "starts": (0xB08FD94, 0x8C)},
        # the loader's temporary buffer (Hunk_AllocateTempMemory(size), 0x59dec0) comes from
        # the hunk the encrypted start-up code set up: from the harness's heap instead
        "temp_alloc": (0x59DEC0,),
        "Scr_InitSystem": 0x5101E0,
        # GScr_LoadScripts (0x18c1e0): Scr_BeginLoadScripts (0x504bc0), Scr_LoadScript,
        # Scr_GetFunctionHandle(name, id), ..., Scr_EndLoadScripts (0x504cf0)
        "Scr_BeginLoadScripts": 0x504BC0,
        "Scr_LoadScript": 0x504FA0,
        "Scr_GetFunctionHandle": 0x504E30,
        "Scr_EndLoadScripts": 0x504CF0,
        # the loader (0x50e340): DB_FindXAssetHeader(0x31, name), DB_IsXAssetDefault, inflate
        "DB_FindXAssetHeader": 0x3950C0,
        "DB_IsXAssetDefault": 0x3968C0,
        "inflate": 0x396080,
        "asset_scriptfile": 0x31,
        "program_mem": (0xC92EC50, 0x200000),
        "Scr_ExecThread": 0x50F160,
        "Scr_FreeThread": 0x50AAE0,
        # G_RunFrame (0x4225e0): Scr_RunCurrentThreads (0x510470) before and after the entity
        # loop, then VM_SetTime (0x50ffd0)
        "Scr_RunCurrentThreads": 0x510470,
        "VM_SetTime": 0x50FFD0,
        "Sys_Milliseconds": 0x5BDDF0,
        # Q_vsnprintf(buf, size, fmt, va_list) (0x5af170): the static UCRT under it reads its
        # per-thread data through FLS pointers resolved at run time: a Python formatter
        "vsnprintf": 0x5AF170,
        "sprintf": (0x6DD80,),       # CRT sprintf (int-to-string conversion, 0x508440)
        "VM_Execute": 0x510EB0,
        "VM_ExecuteInternal": 0x5110E0,
        "error_landing": 0x513A31,   # setjmp (0x8a30f0) returning non-zero
        "loop_kill": 0x513062,       # OP_jumpback's 5000 ms guard (0x512fee), debugger off
        "longjmp": 0x8A31B0,
        "Com_Error": 0x159860,
        # AllocThread (0x509440) and the childthread allocation (0x509120); OP_endon and the
        # childthread's endon copies go through 0x510dd0 (AllocThread returns to 0x510dff)
        "thread_rets": (0x50946B, 0x509284),
        "not_threads": (0x510DFF,),
        "programBuffer": 0xB1100F8,
        "time": 0xB11009C,
        "levelId": 0xB1100B0,
        "top": 0xB7AE3E0, "outparamcount": 0xB7AE3EC, "function_count": 0xB7AE3D0,
        "localId": 0xB7B8948, "opcode": 0xB7B8968, "codepos": 0xB7B8940,
        "sl_buffer": 0xAC87D00, "sl_stride": 16, "sl_text": 8,
        "obj_info": 0xB138180,
        "child_type": 0x12,
        "builtin_table": 0xAC83818,
        "log_builtin": ("logprint", 0x164),
    },
    "iw7": {
        "title": "Infinite Warfare",
        "timestamp": 1524015717,
        # Scr_InitVariables (0xc09280: the object pools) and Scr_Init (0xc0c110: 0xc091d0, then
        # the VM 0xc0c8f0); the string table and the hunk keep the dump's start-up state
        "boot": (0xC09280, 0xC0C110),
        # scrVarPub's hunk users (program, canonical strings, and a third at 0x665a170) are
        # created by start-up code the dump does not let run; made here as Hunk_UserAlloc
        # (0x510ec0) reads them: {u64 end, u64 committed end, u64 position, ..., u32 flags +0x24},
        # the header at the start of its block
        "hunk_users": (0x665A160, 0x665A168, 0x665A170),
        # Scr_BeginLoadScripts clears the builtin tables (0x60b4d60, 0x60b6690) and registers
        # them again through a run-time interface object (0xb518b0, 0xb518d0) absent in a
        # dump: the registration is skipped and the dump's own tables (filled by the running
        # game) are put back
        "skip_calls": (0xB518B0, 0xB518D0, 0x510AD0),
        # Scr_LoadScript's temporary block (0x510a70: {u64 address, u64 size} from the main
        # hunk's low or high end, whose state is the running game's in the dump): from the
        # harness's heap instead; its release (0x510ad0) skipped
        "temp_block": 0x510A70,
        # the script-string memory tree (MT_AllocIndex 0xbfdd10) is the running game's in the
        # dump, full; its init is encrypted at rest: replicated as IW5's MT_Init (0x297620)
        # does it, at this build's addresses (bitmap 0x65b9600, 0x2000 words; size masks
        # 0x65c1600; search starts 0x65c1614): every node free but node 0
        "mt_reset": {"bitmap": 0x65B9600, "words": 0x2000, "masks": 0x65C1600,
                     "starts": (0x65C1614, 0x8C)},
        "keep_tables": ((0x60B4D60, 0x1930), (0x60B6690, 0x2E58)),
        # ... where the running game had registered only its mode's builtins: every handler
        # the static registration tables hold (phase 2's extraction, tools/re/iw/out/iw7.json:
        # array 1 functions, array 0 methods) is written in as the registrar would
        "builtin_entries": {"json": "iw7.json", "arrays": {1: 0x60B4D58, 0: 0x6076690}},
        "Scr_InitSystem": 0xC0C150,
        "fake_dvars": (0xCEB380,),   # Dvar_RegisterBool ("loading_sre_fatal"), from the VM init
        "vsnprintf": 0xCFAB00,       # Q_vsnprintf (as H1's)
        "sprintf": (0x2E100,),       # CRT sprintf (int-to-string conversion, 0xc03880)
        "Sys_Error": 0xD34180,
        # GScr_LoadScripts (0x495a60): Scr_BeginLoadScripts(0) (0xbfd500), Scr_LoadScript
        # (0xbfd900), Scr_GetFunctionHandle(name, id) (0xbfd780); no Scr_EndLoadScripts call:
        # its effect on the VM (scrCompilePub.script_loading = 0, which the error path tests)
        # replicated
        "Scr_BeginLoadScripts": 0xBFD500,
        "Scr_LoadScript": 0xBFD900,
        "Scr_GetFunctionHandle": 0xBFD780,
        "end_load": {"calls": (), "script_loading": 0x60B950A},
        # the loader (0xc09d20): DB_FindXAssetHeader(0x31, name), DB_IsXAssetDefault, inflate
        "DB_FindXAssetHeader": 0xA76E00,
        "DB_IsXAssetDefault": 0xA780D0,
        "inflate": 0xA77AB0,
        "asset_scriptfile": 0x31,
        # Scr_BeginLoadScripts takes programBuffer from 0xa75a80: base 0x53e62e8, size in
        # pages at 0x53e62e0
        "program_mem": (0x53E62E8, 0x200000),
        "program_pages": 0x53E62E0,
        # Scr_ExecThread(handle, paramcount), self = level (0xc0acd0)
        "Scr_ExecThread": 0xC0ACD0,
        "exec_self_arg": False,
        "Scr_FreeThread": 0xC05CA0,
        "Scr_RunCurrentThreads": 0xC0C3E0,
        "VM_SetTime": 0xC0C000,
        "Sys_Milliseconds": 0xD58110,
        "VM_Execute": 0xC0CDB0,
        "VM_ExecuteInternal": 0xC0CFE0,
        "error_landing": 0xC0F89F,   # setjmp (0x1423110) returning non-zero
        "loop_kill": 0xC0EED9,       # OP_jumpback's 5000 ms guard (0xc0ee67), debugger off
        "longjmp": 0x12C0758,
        "Com_Error": 0xB8D830,
        # AllocThread (0xc04580) and the childthread allocation (0xc043a0); OP_endon and the
        # childthread's endon copies go through 0xc0ccd0 (AllocThread returns to 0xc0ccff)
        "thread_rets": (0xC045AB, 0xC04507),
        "not_threads": (0xC0CCFF,),
        "programBuffer": 0x665A178,
        "time": 0x665A10C,
        "levelId": 0x665A120,
        "top": 0x6B183D0, "outparamcount": 0x6B183DC, "function_count": 0x6B183C0,
        "localId": 0x6B22910, "opcode": 0x6B22940, "codepos": 0x6B22908,
        "sl_buffer": 0x60B9588, "sl_stride": 20, "sl_text": 8,
        "obj_info": 0x6691180,
        "child_type": 0x12,
        "builtin_table": 0x60B4D58,
        "log_builtin": ("logprint", 0x131),
    },
}


class IWVM(GameVM):
    def __init__(self, game, exe, log=print, verbose=False):
        super().__init__(exe, log, verbose)
        self.game = game
        self.P = P = PROFILES[game]
        if self.img.timestamp != P["timestamp"]:
            raise SystemExit("%s: PE timestamp %d, the profile is for %d"
                             % (exe, self.img.timestamp, P["timestamp"]))
        self.trace = []
        self.errors = []
        self.thread_names = {}
        self.thread_count = 0
        self.step_count = 30_000_000
        self.frame = 0
        self.time0 = 0
        self.assets = {}
        self.asset_requests = []
        self.stub_next = STUBS + 0x80000
        self.import_handlers["VirtualAlloc"] = lambda vm: vm._virtual_alloc(vm.emu.arg(0),
                                                                            vm.emu.arg(1))
        self.import_handlers["VirtualFree"] = lambda vm: 1
        # a virtual wall clock: +1 ms per read, +50 ms per server frame (the VM's loop guard
        # measures an execution against it)
        self.clock_ms = 0
        self.import_handlers["timeGetTime"] = lambda vm: vm._tick()
        for rva in P.get("fake_dvars", ()):
            self.hook(rva, lambda vm: vm.big_alloc(0x100, 16), "Dvar_Register %#x" % rva,
                      "a zeroed dvar (no dvar system)")
        for rva in P.get("temp_alloc", ()):
            self.hook(rva, lambda vm: vm.big_alloc(max(vm.emu.arg(0), 16), 16),
                      "temp alloc %#x" % rva, "from the harness's heap")
        if P.get("temp_block"):
            def temp(vm):
                out, n = vm.emu.arg(0), (vm.emu.arg(1) + 0x7F) & ~0x7F
                vm.w64(out, vm.big_alloc(n, 0x80))
                vm.w64(out + 8, n)
                return out
            self.hook(P["temp_block"], temp, "temp block", "from the harness's heap")
        for rva in P.get("skip_calls", ()):
            self.hook(rva, lambda vm: 0, "skipped %#x" % rva, "run-time interface absent")
        for rva in P.get("sprintf", ()):
            self.hook(rva, IWVM._sprintf, "sprintf %#x" % rva, "Python formatter")
        if P.get("Sys_Error"):
            self.hook(P["Sys_Error"], IWVM._sys_error, "Sys_Error", "recorded; stops the run")
        if P.get("vsnprintf"):
            self.hook(P["vsnprintf"], GameVM.vsnprintf_hook, "Q_vsnprintf", "Python formatter")
        if P.get("Sys_Milliseconds"):
            self.hook(P["Sys_Milliseconds"], lambda vm: vm._tick(), "Sys_Milliseconds",
                      "the virtual clock")
        if P.get("_getptd_noexit"):
            self.ptd = self.big_alloc(0x800, 16)
            self.hook(P["_getptd_noexit"], lambda vm: vm.ptd, "_getptd_noexit",
                      "one zeroed CRT per-thread block")
        if P.get("crt"):
            import pycrt
            pycrt.install(self)
        self.hook(P["Com_Error"], IWVM._com_error, "Com_Error", "recorded; stops the run")
        if P.get("longjmp"):
            self.hook_longjmp(P["longjmp"])
        self.hook(P["DB_FindXAssetHeader"], IWVM._find_asset, "DB_FindXAssetHeader",
                  "the test scripts' ScriptFile assets only")
        self.hook(P["DB_IsXAssetDefault"], lambda vm: 0, "DB_IsXAssetDefault", "never default")
        self.hook(P["inflate"], IWVM._inflate, "inflate", "Python zlib")

    # --- addresses ---------------------------------------------------------------------------
    def a(self, key):
        return self.B + self.P[key]

    def now(self):
        """scrVarPub.time counted from the entry thread's start (Scr_InitSystem's own
        VM_SetTime pass leaves it at 1 when G_InitGame starts the level's scripts)."""
        return (self.r32(self.a("time")) & 0xFFFFFF) - self.time0

    def reg(self, name):
        return self.uc.reg_read(getattr(X, "UC_X86_REG_" + name.upper()))

    # --- engine services -----------------------------------------------------------------------
    def _com_error(self):
        code, fmt = self.emu.arg(0), self.cstr(self.emu.arg(1), 4096) or ""
        text = self.format(fmt, lambda k: self.emu.arg(2 + k))
        self.prints.append((self.now(), -1, "Com_Error(%d): %s" % (code, text)))
        self.stop_reason = "Com_Error: " + text.strip()
        self.uc.emu_stop()
        return 0

    def _sprintf(self):
        buf, fmt = self.emu.arg(0), self.cstr(self.emu.arg(1), 4096) or ""
        b = self.format(fmt, lambda k: self.emu.arg(2 + k)).encode("latin-1", "replace")
        self.uc.mem_write(buf, b + bytes(1))
        return len(b)

    def _sys_error(self):
        text = self.format(self.cstr(self.emu.arg(0), 4096) or "", lambda k: self.emu.arg(1 + k))
        self.stop_reason = "Sys_Error: " + text.strip()
        if os.environ.get("IWVM_DEBUG"):
            print(self.backtrace())
        self.uc.emu_stop()
        return 0

    def _tick(self):
        self.clock_ms += 1
        return self.clock_ms

    def _virtual_alloc(self, addr, size):
        """Reserve: fresh pages of the big heap; commit inside a reservation: the address."""
        if addr:
            return addr
        return self.big_alloc((size + 0xFFF) & ~0xFFF, 0x10000)

    def add_script(self, name, data):
        """A gsc-tool .gscbin {"GSC\\0", u32 compressedLen, u32 len, u32 bytecodeLen, stack
        (zlib), bytecode} as the ScriptFile asset `name` {name, compressedLen, len,
        bytecodeLen, buffer, bytecode}."""
        if data[:4] != b"GSC\0":
            raise Fault("%s: not a gsc-tool gscbin" % name)
        clen, ln, blen = struct.unpack_from("<III", data, 4)
        stack = data[16:16 + clen]
        code = data[16 + clen:16 + clen + blen]
        a = self.big_alloc(0x28, 16)
        buf = self.big_alloc(len(stack) + 16, 16)
        bc = self.program_next
        self.program_next += (len(code) + 16 + 15) & ~15
        self.uc.mem_write(buf, stack)
        self.uc.mem_write(bc, code)
        self.w64(a, self.put_cstr(name))
        self.w32(a + 8, clen)
        self.w32(a + 0xC, ln)
        self.w32(a + 0x10, blen)
        self.w64(a + 0x18, buf)
        self.w64(a + 0x20, bc)
        self.assets[name.lower()] = a

    def _find_asset(self):
        t, name = self.emu.arg(0) & 0xFFFFFFFF, self.cstr(self.emu.arg(1)) or ""
        self.asset_requests.append((t, name))
        if t == self.P["asset_scriptfile"]:
            key = name.lower().replace("\\", "/")
            return self.assets.get(key[:-4] if key.endswith(".gsc") else key, 0)
        return 0

    def _inflate(self):
        """(stream {?, u32 compressedLen +8, u32 len +0xc, ?, buffer +0x10}, dst, dstlen)."""
        s, dst, n = self.emu.arg(0), self.emu.arg(1), self.emu.arg(2) & 0xFFFFFFFF
        clen = self.r32(s + 8)
        src = self.r64(s + 0x10)
        out = zlib.decompress(bytes(self.uc.mem_read(src, clen)))
        self.uc.mem_write(dst, out[:n])
        return min(len(out), n)

    # --- values ----------------------------------------------------------------------------------
    def sl_string(self, idx):
        stride = self.P.get("sl_stride", 12)
        text = self.P.get("sl_text", 4)
        return self.cstr(self.r64(self.a("sl_buffer")) + stride * idx + text, 4096) or ""

    def value_text(self, va):
        v = self.r64(va)
        t = self.r32(va + 8) & 0x1F
        name = TYPES[t] if t < len(TYPES) else "type%d" % t
        if name == "undefined":
            return "undefined"
        if name in ("string", "istring"):
            return self.sl_string(v & 0xFFFF)
        if name == "int":
            return str(struct.unpack("<i", struct.pack("<I", v & 0xFFFFFFFF))[0])
        if name == "float":
            return "%g" % struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]
        if name == "vector":
            x, y, z = struct.unpack("<fff", bytes(self.uc.mem_read(v, 12)))
            return "(%g, %g, %g)" % (x, y, z)
        return "<%s %#x>" % (name, v)

    def builtin_args(self):
        n = self.r32(self.a("outparamcount"))
        top = self.r64(self.a("top"))
        return [self.value_text(top - 16 * i) for i in range(n)]

    # --- threads ---------------------------------------------------------------------------------
    def obj(self, idx):
        a = self.a("obj_info") + 8 * idx
        w = self.r32(a + 4)
        return w & 0xFF, w >> 8

    def current_thread(self):
        """The root of the running localId's chain of script calls (child threads, type
        child_type, the caller's id above bit 8)."""
        v = self.r32(self.a("localId"))
        for _ in range(256):
            t, parent = self.obj(v)
            if t != self.P["child_type"] or not parent:
                return v
            v = parent
        return v

    def track_threads(self):
        def on(vm):
            tid = vm.reg("eax") & 0xFFFF
            ret = vm.r64(vm.reg("rsp")) - vm.B
            if os.environ.get("IWVM_DEBUG"):
                print("# AllocThread -> %d for %#x" % (tid, ret))
            if ret in vm.P.get("not_threads", ()):
                return
            vm.thread_names[tid] = "T%d" % vm.thread_count
            vm.thread_count += 1
        for rva in self.P["thread_rets"]:
            self.watch(rva, on)

    def thread_name(self, tid):
        return self.thread_names.get(tid, "thread%d" % tid)

    def trace_builtin(self, label, idx):
        """Point builtin function `idx`'s table slot at a stub of ours that records (time,
        thread, label, args): the shared empty handler of the retail build is left alone."""
        stub = self.stub_next
        self.stub_next += 16
        self.uc.mem_write(stub, b"\xc3")
        self._trace_slot = (self.a("builtin_table") + 8 * idx, stub)

        def on(uc, addr, size, ud):
            self.trace.append((self.now(), self.thread_name(self.current_thread()), label,
                               self.builtin_args()))
            self.note("builtin " + label, "recorded in the trace, handler not run")
        self.uc.hook_add(__import__("unicorn").UC_HOOK_CODE, self.guarded(on), begin=stub,
                         end=stub)

    def patch_trace_slot(self):
        slot, stub = self._trace_slot
        self.w64(slot, stub)

    def opcode_names(self):
        """{opcode: name} from gsc-tool's table for the game (its source tree, `gsctool_src` in
        paths.local.json; GPL-3.0, read locally): the names only label the trace."""
        if hasattr(self, "_opnames"):
            return self._opnames
        self._opnames = {}
        from common import paths
        d = paths.get("gsctool_src")
        f = os.path.join(d or "", CODE_TABLE.get(self.game, ""))
        if d and os.path.isfile(f):
            with open(f, encoding="utf-8") as fh:
                for m in re.finditer(r"\{\s*0x([0-9A-Fa-f]+),\s*opcode::(\w+)\s*\}", fh.read()):
                    self._opnames[int(m.group(1), 16)] = m.group(2)
        return self._opnames

    def trace_errors(self, flood=64):
        def on(vm):
            op = vm.r32(vm.a("opcode")) & 0xFF
            name = vm.opcode_names().get(op)
            vm.errors.append((vm.now(), vm.thread_name(vm.current_thread()),
                              "%s (opcode %#x)" % (name, op) if name else "opcode %#x" % op,
                              False, len(vm.trace)))
            if len(vm.errors) >= flood:
                vm.stop_reason = "error flood: %d script errors" % len(vm.errors)
                vm.uc.emu_stop()
        self.watch(self.P["error_landing"], on)
        if self.P.get("loop_kill"):
            def kill(vm):
                vm.errors.append((vm.now(), vm.thread_name(vm.current_thread()),
                                  "loop guard: thread killed", False, len(vm.trace)))
            self.watch(self.P["loop_kill"], kill)

    # --- boot, load, run -------------------------------------------------------------------------
    def boot(self):
        P = self.P
        ptr, size = P["program_mem"]
        self.program_next = self.big_alloc(size, 0x1000)
        self.w64(self.B + ptr, self.program_next)
        if P.get("program_pages"):
            self.w32(self.B + P["program_pages"], size >> 12)
        for rva in P.get("hunk_users", ()):
            size = 0x1000000
            # the header heads its own block (a reset sets the position to header + 0x40)
            u = self.big_alloc(size, 0x1000)
            self.w64(u, u + size)
            self.w64(u + 8, u + size)
            self.w64(u + 0x10, u + 0x40)
            self.w64(self.B + rva, u)
            self.note("hunk user %#x" % rva, "made by the harness (16 MiB, all committed)")
        mt = P.get("mt_reset")
        if mt:
            self.uc.mem_write(self.B + mt["bitmap"], bytes([0xFF]) * (4 * mt["words"]))
            self.w32(self.B + mt["bitmap"], 0x7FFFFFFF)
            for k, v in enumerate((0x80000000, 0xC0000000, 0xF0000000, 0xFF000000, 0xFFFF0000)):
                self.w32(self.B + mt["masks"] + 4 * k, v)
            a, n = mt["starts"]
            self.uc.mem_write(self.B + a, bytes(n))
            self.note("MT_Init", "replicated (encrypted in the dump)")
        for k in P["boot"]:
            self.call(P[k] if isinstance(k, str) else k, count=2_000_000_000)
            self.boot_log.append(k if isinstance(k, str) else "%#x" % k)

    def load(self, name):
        P = self.P
        kept = [(rva, bytes(self.uc.mem_read(self.B + rva, n)))
                for rva, n in P.get("keep_tables", ())]
        self.call(P["Scr_BeginLoadScripts"])
        for rva, data in kept:
            self.uc.mem_write(self.B + rva, data)
        be = P.get("builtin_entries")
        if be:
            import json
            f = os.path.join(os.path.dirname(HERE), "iw", "out", be["json"])
            with open(f) as fh:
                ents = json.load(fh)["entries"]
            n = 0
            for e in ents:
                m = re.search(r"into array ([0-9]+)", e.get("where", ""))
                if m and int(m.group(1)) in be["arrays"] and e.get("id") is not None:
                    self.w64(self.B + be["arrays"][int(m.group(1))] + 8 * e["id"],
                             int(e["handler"], 16))
                    n += 1
            self.note("builtin registration", "%d handlers from %s" % (n, be["json"]))
        self.patch_trace_slot()
        return self.call(P["Scr_LoadScript"], self.put_cstr(name), count=self.step_count)

    def function_handle(self, script, func_id):
        return self.call(self.P["Scr_GetFunctionHandle"], self.put_cstr(script), func_id)

    def end_load(self):
        P = self.P
        if P.get("Scr_EndLoadScripts"):
            self.call(P["Scr_EndLoadScripts"])
        else:
            # inlined in this build: its steps replicated (profile "end_load")
            for fn, args in P["end_load"]["calls"]:
                self.call(fn, *args)
            self.w8(self.B + P["end_load"]["script_loading"], 0)
            self.note("Scr_EndLoadScripts", "inlined in this build: its steps replicated")
        self.call(self.P["Scr_InitSystem"])
        self.boot_log.append("level %d time %d" % (self.r32(self.a("levelId")), self.now()))

    def exec_thread(self, handle):
        level = self.r32(self.a("levelId"))
        self.time0 = self.r32(self.a("time")) & 0xFFFFFF
        if self.P.get("exec_self_arg", True):
            tid = self.call(self.P["Scr_ExecThread"], handle, level, 0, count=self.step_count)
        else:
            tid = self.call(self.P["Scr_ExecThread"], handle, 0, count=self.step_count)
        tid &= 0xFFFF
        self.call(self.P["Scr_FreeThread"], tid)
        return tid

    def run_frame(self):
        P = self.P
        self.frame += 1
        self.clock_ms += 50
        self.call(P["Scr_RunCurrentThreads"], count=self.step_count)
        self.call(P["Scr_RunCurrentThreads"], count=self.step_count)
        self.call(P["VM_SetTime"], count=self.step_count)
        if P.get("frame_tail"):
            self.call(P["frame_tail"], count=self.step_count)
