"""The Black Ops 4 (T8) and Cold War (T9) script VMs run offline under Unicorn, from the flat
dumps: the game's own string table, variable system, VM init, global objects, loader and linker
(`Scr_GscLink`, the asset lookup hooked), `Scr_ExecThread`, the frame functions and every opcode
handler. Only what the VM needs from the rest of the engine is stubbed, and every stub is logged.

Addresses are RVAs of the dumps named by `exe.t8` / `exe.t9` (PE timestamps in PROFILES); every
name is a hypothesis checked by running, see the comments and reports/vm_t8.md, vm_t9.md.
T8/T9 identifiers are 32-bit hashes (`t89.scr`), script names 64-bit (`t8.fnv64`).

Research tool only (Unicorn is GPL-2.0-only, imported from Python like the rest of tools/re).
"""
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
from common.hashes import t89_scr, t8_fnv64  # noqa: E402
from common.emu import X  # noqa: E402
from vmcore import GameVM, Fault  # noqa: E402,F401

OUT = os.path.join(os.path.dirname(HERE), "out")

# --- per-game anchors (RVAs) ------------------------------------------------------------------
PROFILES = {
    "t8": {
        "timestamp": 1780932985,
        # PMem-style allocator (size, align, ...): ScrVar_Init allocates its pools through it
        "alloc": 0x3C3FEA0,
        # CRT rand(): the string table obfuscates script strings with a random key per string
        "rand": 0x4856D58,
        # CRT __stdio_common_vsnprintf_s(options, buf, size, maxcount, fmt, locale, va_list):
        # its locale and per-thread data are not set up in a dump (snprintf 0x4f9270 calls it
        # when a hash is cast to a string, "%Ix")
        "vsnprintf_s": 0x4855608,
        # CRT __stdio_common_vsprintf(options, buf, size, fmt, locale, va_list): va() (0x3d12860)
        # formats the runtime error messages with it
        "vsprintf": 0x4855754,
        # CRT longjmp(jmp_buf, value): ScrVm_Error's way back into VM_Execute's recovery
        "longjmp": 0x4844058,
        # (dst, len, src, key): writes `src` obfuscated (the first byte tags the cipher); the
        # reader 0x3c4c570 decodes in place. Replaced by a plain copy (strings stay readable;
        # the hash the table keys on is computed before, from the clear text)
        "sl_obfuscate": 0x3C52500,
        # SL_Init (no direct caller: a startup table): MT_Init (0x2744610: the node pool, its
        # pointer at 0x7dad620) and the 0x4000-bucket hash table at 0x82f5230
        "SL_Init": 0x2757220,
        # ScrVar_Init(inst): five pools (name hash, variables 0x20 x 160000, object info 1/2,
        # values 0x10 x 160000) into scrVarGlob[inst] (0x8305230, stride 0x28), then the free
        # lists (tail call 0x275e280). Its one caller (0x2868ad0) inits inst 1 (client)
        "ScrVar_Init": 0x275DFF0,
        # Scr_InitVM(inst): VM stack (top), function frames, the error jmp_buf, the execution
        # queues' heap; sets scrVmPub+0x881c (initialised)
        "Scr_InitVM": 0x27737F0,
        # Scr_StartVM(inst, persist_game, levelTime): global objects world, mission, level, game,
        # anim, classes (+ one more) in gScrVarPub.globalVars, time = levelTime, frame = 0 and the
        # running flag scrVmPub+0x881d that the frame functions test (G_InitGame, 0x2cf0424)
        "Scr_StartVM": 0x2773990,
        # Scr_BeginLoadScripts(inst) / Scr_EndLoadScripts(inst, ?): GScr_LoadScripts (0x339c410)
        # brackets its links with them
        "Scr_BeginLoadScripts": 0x2743D60,
        "Scr_EndLoadScripts": 0x2743E30,
        # Scr_GscLink(inst, XHash *name, runScript): asset lookup, load (+ includes), link; with
        # runScript the autoexec exports run
        "Scr_GscLink": 0x2746EB0,
        # DB_FindXAssetHeader(type, XHash *name, errorIfMissing, wait): hooked
        "DB_FindXAssetHeader": 0x2EB58B0,
        "asset_scriptparsetree": 0x30,
        # Scr_ExecThread(inst, codepos, paramcount, retval*, self): AllocThread then
        # VM_ExecuteInternal (0x276efc0)
        "Scr_ExecThread": 0x276E9E0,
        # VM_SetTime(inst, levelTime): resumes the due time/frame queue threads, frame += 1,
        # time = levelTime, then the timeouts; Scr_RunCurrentThreads(inst): the due threads again.
        # G_RunFrame (0x2d072c0) calls SetTime then RunCurrentThreads first, RunCurrentThreads
        # again at its end (0x2d10002)
        "VM_SetTime": 0x2773570,
        "Scr_RunCurrentThreads": 0x2775700,
        # ScrVm_Error(msg hash, inst, terminal): the runtime error entry (VM_Execute's invalid
        # opcode path calls it with 0xf14397e0); reports (0x27758b0), then longjmps unless terminal
        "ScrVm_Error": 0x276E530,
        # VM_Notify core (phase 3 events): (inst, object, event, top...)
        "VM_Notify": 0x2777430,
        # AllocThread(inst, self): its call sites in the thread-call handlers, Scr_Exec*Thread and
        # the notify callback runner (0x2774090)
        "AllocThread": 0x2758AB0,
        "VM_Execute": 0x276ECE0,
        # data
        "gScrVarPub": 0x8305280, "gScrVarPub_stride": 0x110,
        "globalVars": 0x28,                       # 16 x {u32 name, u32 id, u8 persist} (12 bytes)
        # variables: scrVarGlob[inst] (stride 0x28): +8 ScrVar_t[] (0x20: parent id at +0x14),
        # +0x20 values[] (0x10: type at +8)
        "vars": {"glob": 0x8305230, "stride": 0x28},
        # scrVmPub[inst] is image data here (stride 0x16868), its top at 0x83134e8
        "vm": {"stride": 0x16868, "top": 0x83134E8, "outparamcount": 0x831BD00,
               "time": 0x831BCF0, "frame": 0x831BCF4, "fs": 0x833A5D0, "fs_stride": 0x20},
        "obj_info": 0x82ED6D0, "obj_info_stride": 0x18, "obj_info_per_inst": 650,
        "obj_count": 0x82F50B0,
        # GSC_OBJ (VM 0x36): name +0x10, exports_count u16 +0x1e, export table +0x30
        "obj": {"name": 0x10, "exports_count": 0x1E, "exports": 0x30},
        "sl_pool_ptr": 0x7DAD620, "sl_node": 20, "sl_text": 0x10,
        # ScriptParseTree {XHash name (16 bytes), buffer, len}; the lookup takes an XHash *
        "spt": {"size": 0x20, "buffer": 0x10, "len": 0x18, "hash_by_pointer": True},
        "types": "t8",
        "builtins": "t8_extract.json",
    },
    "t9": {
        "timestamp": 1778629159,
        # CRT cores, reached through thunks (0xd46906c -> vsnprintf_s, 0xd469314 -> vsprintf)
        "vsnprintf_s": 0xD46906C,
        "vsprintf": 0xD469314,
        "longjmp": 0xD451410,
        # CRT __isa_available: 5 (AVX2) in the dump, taken from the running game; memmove
        # (0xd450b40) then uses AVX, which Unicorn does not run: set to 1 (SSE2)
        "isa_available": 0xECE0158,
        # (dst, len, src, key): the string table stores script strings obfuscated (decoded in
        # place on read, 0xc867390); replaced by a plain copy, as in T8
        "sl_obfuscate": 0xC8704A0,
        # acts writes each string of a CW object tagged {0x8b, length + 1, 0, text in clear}; the
        # game copies a tagged string with strcpy (0x1a3c640: the 0 byte cuts it) and decodes it
        # with one of 64 ciphers (0xc86bc30), so add_script turns the tags back into clear text
        # (the same object as acts' T8 output) before the game loads it
        "untag_strings": {"count": 0x18, "table": 0x30},
        # MT_Init is obfuscated (its tail is a chunk in the second .text, 0x1befa645, reached
        # through a pushed address): replicated here: both pool pointers (0xfed8d78, 0xfed8d70)
        # = the static node pool 0xeed8cd0, flag byte 0xfed8d80 = (b & 0xd1) | 0x51, then the
        # tail call MT_Alloc(0x10, 0) (0x1b5da20)
        "mt_init": {"pool_ptrs": (0xFED8D78, 0xFED8D70), "pool": 0xEED8CD0, "flag": 0xFED8D80,
                    "mt_alloc": 0x1B5DA20},
        # ScrVar_Init(inst): paged variable storage ("pg%d%d"), page table at 0x10022760
        # (stride 0x228). G_InitGame's server path (0x5699100) calls it with 0
        "ScrVar_Init": 0x1B88040,
        # the memory system's allocators (size, align, ...) used by ScrVar_Init: its pages
        # (0x9f32160) and name hash list (0x9f32260) come from a "scrmem" pool of the "main"
        # allocator set up at engine start (absent here: Sys_Error 0x8af19f48 ",scrmem,main");
        # replaced by the bump heap, and the pool's reserve(pool, size) (0x9f32130, a bool) by
        # "reserved"
        "allocs": (0x9F32160, 0x9F32260),
        "returns_1": (0x9F32130,),
        "Scr_InitVM": 0x1BB4E60,
        "Scr_StartVM": 0x1BB4F70,
        # GScr_LoadScripts (0x3c80db0): BeginLoad(0), links, EndLoad(0, 1)
        "Scr_BeginLoadScripts": 0x1B5D4C0,
        "Scr_EndLoadScripts": 0x1B5D580,
        # Scr_GscLink(inst, name hash (by value), runScript): obfuscated entry (an opaque jns),
        # run as is
        "Scr_GscLink": 0x1B60320,
        # DB_FindXAssetHeader(type, name hash, errorIfMissing, wait); ScriptParseTree is type 0x44
        "DB_FindXAssetHeader": 0xB1E78F0,
        "asset_scriptparsetree": 0x44,
        "Scr_ExecThread": 0x1BB0B60,
        # G_RunFrame (0x64cc4f0): VM_SetTime(0, level.time), Scr_RunCurrentThreads(0), ...,
        # Scr_RunCurrentThreads(0) again (0x64def77)
        "VM_SetTime": 0x1BB4C90,
        "Scr_RunCurrentThreads": 0x1BB77F0,
        "ScrVm_Error": 0x1BB06E0,
        "VM_Notify": 0x1BB97A0,
        "AllocThread": 0x1B78150,
        "VM_Execute": 0x1BB0DF0,
        "gScrVarPub": 0x10022530, "gScrVarPub_stride": 0x110,
        "globalVars": 0x28,
        # variables in 32 pages: page id & 0x1f at 0x10022760 + inst * 0x228 + page * 0x10 (+8:
        # base); 0x2222 variables a page: value (id >> 5) * 0x10 from the base, ScrVar_t at
        # +0x22220 + (id >> 5) * 0x20 (ScrVar_InitVariables 0x1b77bc0)
        "vars": {"pages": 0x10022760, "stride": 0x228, "var_off": 0x22220},
        # scrVmPub[inst] (0x10022bc0, stride 0x1cbe70): time +0xa804c, frame +0xa8050, a pointer
        # at +0xa8058 to the VM stack block: top +8, function_stack_t +0x20, outparamcount +0xe96c
        "vm": {"stride": 0x1CBE70, "ptr": 0x100CAC18, "top": 0x8, "outparamcount": 0xE96C,
               "time": 0x100CAC0C, "frame": 0x100CAC10, "fs": 0x20},
        "obj_info": 0xFFD8D80, "obj_info_stride": 0x18, "obj_info_per_inst": 800,
        "obj_count": 0xFFE2380,
        # GSC_OBJ (VM 0x38): name +0x10, exports_count u16 +0x1a, export table +0x38
        "obj": {"name": 0x10, "exports_count": 0x1A, "exports": 0x38},
        # string table nodes of 16 bytes, the text at +0x18 of the entry (length at +4: the
        # concatenation 0x1b74ef0)
        "sl_pool_ptr": 0xFED8D78, "sl_node": 16, "sl_text": 0x18,
        # ScriptParseTree {u64 name, buffer, len}; the lookup takes the hash by value
        "spt": {"size": 0x18, "buffer": 0x8, "len": 0x10, "hash_by_pointer": False},
        "types": "t9",
        "builtins": "t9_extract.json",
    },
}

# VM value types, in each game's own type-name table order (phase 2: T8 0x4eeb240, T9 0xe773570)
TYPES = {
    "t8": ("undefined pointer string vector hash float int uintptr entity_offset codepos precodepos "
           "api_function function stack thread notify_thread time_thread frame_thread child_thread "
           "class shared_struct struct removed_entity entity array removed_thread free thread_list "
           "ent_list"),
    "t9": ("undefined pointer string vector hash float int finalization uintptr entity_offset "
           "codepos precodepos api_function function stack thread notify_thread time_thread "
           "frame_thread child_thread removed_thread array class shared_struct struct entity "
           "removed_entity free thread_list ent_list"),
}
VT_NAMES = dict(enumerate(TYPES["t8"].split()))  # T8's, kept for callers of the module
ERROR_FLOOD = 64


class T89VM(GameVM):
    def __init__(self, game, exe, log=print, verbose=False):
        super().__init__(exe, log, verbose)
        self.game = game
        self.P = P = PROFILES[game]
        if self.img.timestamp != P["timestamp"]:
            raise SystemExit("%s: PE timestamp %d, the profile is for %d"
                             % (exe, self.img.timestamp, P["timestamp"]))
        self.inst = 0
        self.trace = []
        self.errors = []
        self.thread_names = {}
        self.thread_count = 0
        self.step_count = 20_000_000
        self.frame_ms = 50
        self.level_time = 0
        self.frames_run = 0
        self.assets = {}
        self.asset_requests = []
        self.rand_state = 1
        self.vt_names = dict(enumerate(TYPES[P["types"]].split()))
        self.vt = {v: k for k, v in self.vt_names.items()}

    # --- addresses ---------------------------------------------------------------------------
    def a(self, key):
        return self.B + self.P[key]

    def vm_addr(self, field):
        """A scrVmPub field: image data (T8), or behind the per-inst pointer (T9) for top,
        outparamcount and fs; time and frame are image data in both."""
        V = self.P["vm"]
        if "ptr" in V and field in ("top", "outparamcount", "fs"):
            return self.r64(self.B + V["ptr"] + V["stride"] * self.inst) + V[field]
        if field == "fs" and "fs_stride" in V:
            return self.B + V["fs"] + V["fs_stride"] * self.inst
        return self.B + V[field] + V["stride"] * self.inst

    def pub(self, off):
        return self.a("gScrVarPub") + self.P["gScrVarPub_stride"] * self.inst + off

    # --- boot ----------------------------------------------------------------------------------
    def boot(self):
        P = self.P
        self._builtin_indexes()
        if "isa_available" in P:
            self.w32(self.B + P["isa_available"], 1)
            self.note("__isa_available", "set to 1 (SSE2): Unicorn has no AVX")
        for i, rva in enumerate(P.get("allocs", ()) + ((P["alloc"],) if "alloc" in P else ())):
            self.hook(rva, lambda vm: vm.big_alloc(vm.emu.arg(0), max(vm.emu.arg(1), 16)),
                      "alloc %#x" % rva, "bump heap, never freed")
        for rva in P.get("returns_1", ()):
            self.hook(rva, lambda vm: 1, "stub %#x" % rva, "returns 1 at once")
        if "rand" in P:
            self.hook(P["rand"], T89VM._rand, "rand", "CRT rand, MS LCG from seed 1 (deterministic)")
        self.hook(P["vsnprintf_s"], lambda vm: vm._crt_printf(1, 2, 4, 6), "vsnprintf_s",
                  "Python formatter")
        self.hook(P["vsprintf"], lambda vm: vm._crt_printf(1, 2, 3, 5), "vsprintf",
                  "Python formatter")
        self.hook_longjmp(P["longjmp"])
        if "sl_obfuscate" in P:
            self.hook(P["sl_obfuscate"], T89VM._sl_copy, "SL_Obfuscate",
                      "plain copy: script strings are kept readable in the string table")
        self.hook(P["DB_FindXAssetHeader"], T89VM._find_asset, "DB_FindXAssetHeader",
                  "returns the test scripts' ScriptParseTree; any other asset: not found")
        if "SL_Init" in P:
            self.call(P["SL_Init"])
        else:
            M = P["mt_init"]
            for ptr in M["pool_ptrs"]:
                self.w64(self.B + ptr, self.B + M["pool"])
            f = self.B + M["flag"]
            self.w8(f, (self.r8(f) & 0xD1) | 0x51)
            self.call(M["mt_alloc"], 0x10, 0)
            self.note("MT_Init", "replicated (obfuscated in the dump): pool pointers, flag, MT_Alloc(0x10)")
        self.call(P["ScrVar_Init"], self.inst)
        self.call(P["Scr_InitVM"], self.inst)
        self.call(P["Scr_StartVM"], self.inst, 0, self.level_time)

    def _builtin_indexes(self):
        """The large builtin tables are searched with bsearch over an index array sorted by hash;
        the arrays are zero in the dumps (filled at run time). Fill each with the hash order of its
        table (the only order the comparator accepts); places from phase 2's out/<game>_extract.json."""
        p = os.path.join(OUT, self.P["builtins"])
        if not os.path.exists(p):
            raise SystemExit("needs %s: run `py -3.11 -I treyarch/t89_builtins.py extract --game %s`"
                             % (p, self.game))
        with open(p) as f:
            d = json.load(f)
        if d["pe_timestamp"] != self.img.timestamp:
            raise SystemExit("%s is for PE timestamp %d" % (p, d["pe_timestamp"]))
        self.builtin_tables = {}
        n = 0
        for role, lk in d["lookups"].items():
            for t in lk["tables"]:
                hs = [struct.unpack_from("<I", self.img.d, t["rva"] + 0x20 * k)[0]
                      for k in range(t["count"])]
                self.builtin_tables.setdefault(role, []).append((t["rva"], hs))
                arr = t.get("index_array_rva")
                if arr:
                    perm = sorted(range(t["count"]), key=lambda k: (hs[k], k))
                    cur = struct.unpack_from("<%di" % t["count"], self.img.d, arr)
                    if any(cur) and list(cur) != perm:
                        raise SystemExit("index array %#x is filled, not in hash order" % arr)
                    self.uc.mem_write(self.B + arr, struct.pack("<%di" % t["count"], *perm))
                    n += 1
        self.boot_log.append("builtin bsearch index arrays filled: %d" % n)

    def builtin_handler(self, name, role="gsc_function"):
        """The handler RVA of a builtin, read off the static tables (first table of the lookup
        that has its hash, as the lookup returns it)."""
        h = t89_scr(name)
        for rva, hs in self.builtin_tables[role]:
            if h in hs:
                k = hs.index(h)
                return struct.unpack_from("<Q", self.img.d, rva + 0x20 * k + 0x10)[0] - self.B
        return None

    def _rand(self):
        self.rand_state = (self.rand_state * 0x343FD + 0x269EC3) & 0xFFFFFFFF
        return (self.rand_state >> 16) & 0x7FFF

    def _crt_printf(self, i_buf, i_size, i_fmt, i_va):
        """A CRT __stdio_common_v*printf* core: buffer, size, format and va_list by position."""
        buf, size = self.emu.arg(i_buf), self.emu.arg(i_size)
        fmt, va = self.emu.arg(i_fmt), self.emu.arg(i_va)
        text = self.format(self.cstr(fmt, 4096) or "", lambda k: self.r64(va + 8 * k))
        b = text.encode("latin-1", "replace")
        if size:
            n = min(len(b), size - 1)
            self.uc.mem_write(buf, b[:n] + bytes(1))
        return len(b) if len(b) < size else -1

    def _sl_copy(self):
        dst, n, src = self.emu.arg(0), self.emu.arg(1) & 0xFFFFFFFF, self.emu.arg(2)
        s = (self.cstr(src, max(n, 1)) or "").encode("latin-1")
        self.uc.mem_write(dst, s[:max(n - 1, 0)] + b"\0")
        return dst

    # --- load ----------------------------------------------------------------------------------
    def add_script(self, name, data):
        """Make `name` (scripts/zz_harness/x.gsc) findable by the asset lookup: a ScriptParseTree
        {XHash name (16 bytes), buffer, len}."""
        h = t8_fnv64(name)
        S = self.P["spt"]
        if "untag_strings" in self.P:
            data = self.untag(data, self.P["untag_strings"])
        buf = self.big_alloc(len(data) + 16, 16)
        self.uc.mem_write(buf, data)
        spt = self.big_alloc(S["size"], 16)
        self.w64(spt, h)
        self.w64(spt + S["buffer"], buf)
        self.w32(spt + S["len"], len(data))
        self.assets[h] = (name, spt)
        return spt

    @staticmethod
    def untag(data, U):
        """The object with each tagged string {tag 0x80-0xbf, length + 1, 0, text} of its string
        table rewritten in place as the clear text (NUL padded): the table's string items
        {u32 offset, u8 refs, u8 type, u16} + refs x u32 are walked from the header."""
        d = bytearray(data)
        n = struct.unpack_from("<H", d, U["count"])[0]
        o = struct.unpack_from("<I", d, U["table"])[0]
        for _ in range(n):
            s, refs = struct.unpack_from("<IB", d, o)
            o += 8 + 4 * refs
            if d[s] & 0xC0 == 0x80:
                k = max(d[s + 1] - 1, 0)
                text = bytes(d[s + 3:s + 3 + k])
                d[s:s + 3 + k] = text + bytes(3)
        return bytes(d)

    def _find_asset(self):
        t, h = self.emu.arg(0) & 0xFF, self.emu.arg(1)
        if self.P["spt"]["hash_by_pointer"]:
            h = self.r64(h)
        h &= 0x7FFFFFFFFFFFFFFF
        self.asset_requests.append((t, h))
        if t == self.P["asset_scriptparsetree"] and h in self.assets:
            return self.assets[h][1]
        return 0

    def begin_load(self):
        self.call(self.P["Scr_BeginLoadScripts"], self.inst)

    def link(self, name):
        """Scr_GscLink(inst, &XHash(name), runScript 0)."""
        h = t8_fnv64(name)
        if self.P["spt"]["hash_by_pointer"]:
            xh = self.big_alloc(0x10, 16)
            self.w64(xh, h)
            h = xh
        self.call(self.P["Scr_GscLink"], self.inst, h, 0, count=self.step_count)

    def end_load(self):
        self.call(self.P["Scr_EndLoadScripts"], self.inst, 1)

    def loaded_objects(self):
        P = self.P
        n = self.r32(self.a("obj_count") + 4 * self.inst)
        base = self.a("obj_info") + P["obj_info_stride"] * P["obj_info_per_inst"] * self.inst
        return [self.r64(base + P["obj_info_stride"] * k) for k in range(n)]

    def function_handle(self, script, namespace, func):
        """The code address of an export of a linked object, read off its export table
        (GSC_EXPORT_ITEM {checksum, address, name, namespace, callback, params, flags}, 0x18)."""
        h = t8_fnv64(script)
        fn, ns = t89_scr(func), t89_scr(namespace)
        O = self.P["obj"]
        for obj in self.loaded_objects():
            if self.r64(obj + O["name"]) & 0x7FFFFFFFFFFFFFFF != h:
                continue
            n = struct.unpack("<H", bytes(self.uc.mem_read(obj + O["exports_count"], 2)))[0]
            et = self.r32(obj + O["exports"])
            for k in range(n):
                _, addr, name, nsp = struct.unpack("<IIII", bytes(self.uc.mem_read(obj + et + 0x18 * k, 16)))
                if name == fn and nsp == ns:
                    return obj + addr
        return 0

    # --- globals and values --------------------------------------------------------------------
    def global_id(self, name):
        h = t89_scr(name)
        for k in range(16):
            o = self.pub(self.P["globalVars"] + 12 * k)
            if self.r32(o) == h:
                return self.r32(o + 4)
        return 0

    def sl_string(self, idx):
        if not idx:
            return ""
        pool = self.r64(self.a("sl_pool_ptr"))
        va = pool + self.P["sl_node"] * idx + self.P["sl_text"]
        head = bytes(self.uc.mem_read(va, 3))
        if head[0] & 0xC0 == 0x80 and "sl_decrypt" in self.P:
            # kept tagged in the table ({tag, length + 1, 0, text}), decoded on each read
            return bytes(self.uc.mem_read(va + 3, max(head[1] - 1, 0))).decode("latin-1")
        return self.cstr(va, 4096)

    def value_text(self, va):
        v, t = struct.unpack("<QI", bytes(self.uc.mem_read(va, 12)))
        VT = self.vt
        if t == VT["undefined"]:
            return "undefined"
        if t == VT["string"]:
            return self.sl_string(v & 0xFFFFFFFF)
        if t == VT["int"]:
            return str(struct.unpack("<q", struct.pack("<Q", v))[0])
        if t == VT["float"]:
            return "%g" % struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]
        if t == VT["hash"]:
            return "#%x" % v
        if t == VT["vector"]:
            x, y, z = struct.unpack("<fff", bytes(self.uc.mem_read(v, 12)))
            return "(%g, %g, %g)" % (x, y, z)
        return "<%s %#x>" % (self.vt_names.get(t, "type%d" % t), v)

    def builtin_args(self):
        n = self.r32(self.vm_addr("outparamcount"))
        top = self.r64(self.vm_addr("top"))
        return [self.value_text(top - 0x10 * i) for i in range(n)]

    def now(self):
        """scrVmPub.frame: the VM's frame counter (VM_SetTime adds one per server frame)."""
        return self.r32(self.vm_addr("frame"))

    def vm_time(self):
        return self.r32(self.vm_addr("time"))

    def _var_slots(self, idx):
        """(value address, ScrVar_t address) of a variable id."""
        V = self.P["vars"]
        if "pages" in V:
            base = self.r64(self.B + V["pages"] + V["stride"] * self.inst + 0x10 * (idx & 0x1F) + 8)
            k = idx >> 5
            return base + 0x10 * k, base + V["var_off"] + 0x20 * k
        glob = self.B + V["glob"] + V["stride"] * self.inst
        return self.r64(glob + 0x20) + 0x10 * idx, self.r64(glob + 8) + 0x20 * idx

    def var_type(self, idx):
        return self.r32(self._var_slots(idx)[0] + 8)

    def var_parent(self, idx):
        return self.r32(self._var_slots(idx)[1] + 0x14)

    def current_thread(self):
        """The running thread: function_stack_t.threadId is the current function's frame; a script
        call runs in a child_thread variable whose parent (ScrVar_t +0x14, measured) is the
        caller's; the thread is the root of that chain."""
        v = self.r32(self.vm_addr("fs") + 0x18)
        for _ in range(256):
            if self.var_type(v) != self.vt["child_thread"]:
                return v
            v = self.var_parent(v)
        return v

    # --- running -------------------------------------------------------------------------------
    def exec_thread(self, handle, self_id):
        return self.call(self.P["Scr_ExecThread"], self.inst, handle, 0, 0, self_id,
                         count=self.step_count)

    def run_frame(self):
        """One server frame as G_RunFrame drives the VM: VM_SetTime(levelTime += frame_ms), then
        Scr_RunCurrentThreads, then (after the entity loop, empty here) Scr_RunCurrentThreads."""
        self.level_time += self.frame_ms
        self.frames_run += 1
        self.call(self.P["VM_SetTime"], self.inst, self.level_time, count=self.step_count)
        self.call(self.P["Scr_RunCurrentThreads"], self.inst, count=self.step_count)
        self.call(self.P["Scr_RunCurrentThreads"], self.inst, count=self.step_count)

    def track_threads(self):
        """Name threads T0, T1... in creation order: the return of AllocThread at each of its
        direct call sites."""
        va = self.a("AllocThread")
        sites = self.img.rel32_callers({va})[va]

        def on(vm):
            tid = vm.uc.reg_read(X.UC_X86_REG_EAX)
            vm.thread_names[tid] = "T%d" % vm.thread_count
            vm.thread_count += 1
        for site, kind in sites:
            if kind == "call":
                self.watch(site + 5 - self.B, on)
        self.boot_log.append("AllocThread call sites watched: %d" % len(sites))

    def thread_name(self, tid):
        return self.thread_names.get(tid, "thread%d" % tid)

    def trace_builtin(self, rva, label):
        def on(vm):
            vm.trace.append((vm.now(), vm.thread_name(vm.current_thread()), label,
                             vm.builtin_args()))
            return 0
        self.hook(rva, on, "builtin " + label, "recorded in the trace, handler not run")

    def trace_errors(self):
        """Each runtime script error at ScrVm_Error's entry: (frame, thread, message hash,
        terminal); the game's own handling then runs."""
        def on(vm):
            vm.errors.append((vm.now(), vm.thread_name(vm.current_thread()),
                              vm.emu.arg(0) & 0xFFFFFFFF, bool(vm.emu.arg(2) & 0xFF), len(vm.trace)))
            if len(vm.errors) >= ERROR_FLOOD:
                vm.stop_reason = "error flood: %d script errors" % len(vm.errors)
                vm.uc.emu_stop()
        self.watch(self.P["ScrVm_Error"], on)
