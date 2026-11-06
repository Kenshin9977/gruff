"""Trace mode for the emulated Black Ops III (T7) script VM: records a gsc-trace (trace.py,
docs/trace-format.md) of a run from hooks on the game's own code.

Where each event comes from (RVAs of the dump named by exe.t7, PE timestamp 1765634846):

- every opcode: the two dispatch calls of VM_Execute (0x12edd2c table 0x3287350 for opcodes
  with bit 13, 0x12edd36 table 0x3267350) and the two of the error-recovery re-dispatch
  0x12ee0f0 (0x12ee1f2 / 0x12ee1fc, tables 0x3297350 / 0x3277350: each opcode's error variant).
  At a dispatch, function_state.pos is past the u16 opcode; the opcode is named by the handler
  the normal table gives for it (HANDLERS: acts' T7 opcode names, matched by table, phase 5.0).
  Waits, waittills, endons, notifies, builtin calls and thread calls are decoded here from
  their operands and the VM stack, before the handler runs.
- thread creation: ScrVar_AllocThread's return at the thread-call handlers and Scr_Exec*Thread
  (t7vm.ALLOC_THREAD_SITES); the entry function is where the new thread's first opcode is.
- thread end: 0x12e0c60(inst, id), which turns the thread variable into removed_thread (0x1a);
  measured: it runs for a thread that ends and for one an endon kills (inside the Notify
  handler), never for a thread that only waits (that one becomes a time_thread, 0x13).
- wait end: the waiting thread's next opcode.
- script errors: Scr_Throw (0x12ea4e0), with gScrVarPub.error_message and the terminal flag.

Source positions: code addresses are mapped to (script, function, offset) with the exports of
each loaded .gscc, and to source lines with the line table the official linker writes beside it
(<path>.gsc.gdb, parse_gdb).
"""
import importlib.util
import os
import struct

from t7vm import VT, VT_NAMES, VM_FS
from trace import SourceMap

GSCC_TABLES = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__)))), "linker", "gscc_imports.py")

DISPATCH = {0x12EDD2C: False, 0x12EDD36: False, 0x12EE1F2: True, 0x12EE1FC: True}  # rva: recovery
OPTAB = {False: 0x3267350, True: 0x3287350}  # normal handler table without / with bit 13
THREAD_REMOVE = 0x12E0C60
# handler RVA -> opcode name (acts gsc_opcodes_t7_1c names; each handler has one name)
HANDLERS = {
    0x12CCEC0: "Bit_And", 0x12CCF00: "Bit_Or", 0x12CCF40: "Bit_Xor", 0x12CCF80: "BoolComplement",
    0x12CCFA0: "BoolNot", 0x12CCFC0: "CastBool", 0x12CCFE0: "Dec", 0x12CD0F0: "DevblockBegin",
    0x12CD190: "Divide", 0x12CD1D0: "EndSwitch", 0x12CD250: "Equal", 0x12CD290: "GetByte",
    0x12CD2D0: "GetFloat", 0x12CD320: "GetHash32", 0x12CD380: "IW_GetIString",
    0x12CD3D0: "GetInteger", 0x12CD410: "GetNegByte", 0x12CD450: "GetNegUnsignedShort",
    0x12CD4A0: "GetString", 0x12CD4F0: "GetUIntPtr", 0x12CD540: "GetUndefined",
    0x12CD570: "GetShort", 0x12CD5B0: "GetVector", 0x12CD5F0: "GetZero",
    0x12CD640: "GreaterThanOrEqualTo", 0x12CD6A0: "GreaterThan", 0x12CD6E0: "Inc",
    0x12CD800: "JumpOnFalseExpr", 0x12CD870: "JumpOnFalse", 0x12CD8E0: "JumpOnTrueExpr",
    0x12CD950: "JumpOnTrue", 0x12CD9B0: "Jump", 0x12CDA00: "LessThanOrEqualTo",
    0x12CDA60: "LessThan", 0x12CDAA0: "Minus", 0x12CDAE0: "Modulus", 0x12CDB20: "Multiply",
    0x12CDB80: "NotEqual", 0x12CDBC0: "Plus", 0x12CDC00: "ShiftLeft", 0x12CDC40: "ShiftRight",
    0x12CDCA0: "SuperEqual", 0x12CDD00: "SuperNotEqual", 0x12CDDA0: "Switch",
    0x12CDF90: "VectorConstant", 0x12CE070: "VectorScale", 0x12CE1A0: "Vector",
    0x12CE3C0: "CallBuiltinMethod", 0x12CE480: "CallBuiltinFunction",
    0x12CE560: "CheckClearParams", 0x12CE5A0: "ClearParams", 0x12CE600: "End",
    0x12CE720: "PreScriptCall", 0x12CE750: "Return", 0x12CE890: "SafeCreateLocalVariables",
    0x12CEB40: "ClassFunctionCall", 0x12CECD0: "ScriptFunctionCallPointer",
    0x12CEEA0: "ScriptFunctionCall", 0x12CEFC0: "ScriptMethodCallPointer",
    0x12CF1F0: "ScriptMethodCall", 0x12CF370: "ScriptMethodThreadCallPointer",
    0x12CF590: "ScriptMethodThreadCall", 0x12CF760: "ClassFunctionThreadCall",
    0x12CF950: "ScriptThreadCallPointer", 0x12CFB30: "ScriptThreadCall", 0x12CFE30: "Abort",
    0x12CFE50: "AutoBreakpoint", 0x12CFE70: "Breakpoint", 0x12CFE90: "ErrorBreakpoint",
    0x12CFEB0: "EvalLocalVariableCachedDebug", 0x12CFF30: "EvalLocalVariableRefCachedDebug",
    0x12CFFA0: "Nop", 0x12CFFC0: "NotifyBreakpoint", 0x12CFFE0: "T7_ProfileStart",
    0x12D0050: "PushEntityVar", 0x12D0070: "PushVar", 0x12D0090: "ThreadEndBreakpoint",
    0x12D00B0: "WatchBreakpoint", 0x12D0120: "CastFieldObject", 0x12D0180: "ClearArray",
    0x12D01C0: "ClearFieldVariable", 0x12D0200: "DecTop", 0x12D0230: "EmptyArray",
    0x12D02D0: "EvalArrayRef", 0x12D05F0: "EvalArray", 0x12D0660: "EvalFieldVariableRef",
    0x12D06C0: "EvalFieldVariable", 0x12D0720: "EvalLocalVariableCached",
    0x12D0790: "EvalLocalVariableRefCached", 0x12D07D0: "FirstArrayKey", 0x12D08B0: "GetFunction",
    0x12D0900: "GetAnimGRef", 0x12D0930: "IW_GetAnim", 0x12D0980: "IW_GetAnimation",
    0x12D09D0: "GetClassesObject", 0x12D0A00: "GetClasses", 0x12D0A50: "GetResolveFunction",
    0x12D0AA0: "IW_GetGameRef", 0x12D0AD0: "IW_GetGame", 0x12D0B30: "IW_GetLevelGRef",
    0x12D0B60: "IW_GetLevel", 0x12D0BF0: "GetSelfObject", 0x12D0C90: "GetSelf",
    0x12D0CF0: "GetTime", 0x12D25C0: "GetWorldObject", 0x12D25F0: "GetWorld",
    0x12D2640: "IsDefined", 0x12D2740: "IW_EvalLevelFieldVariableRef",
    0x12D2860: "IW_EvalLevelFieldVariable", 0x12D2950: "GetObjectType", 0x12D2A60: "NextArrayKey",
    0x12D2B70: "IgnoreWaittillVariableFieldCached",
    0x12D2C20: "IW_SetWaittillVariableFieldCached", 0x12D2D20: "IW_EvalSelfFieldVarRef",
    0x12D2E60: "IW_EvalSelfFieldVar", 0x12D2F90: "SetVariableField", 0x12D3020: "SizeOf",
    0x12D3CA0: "IW_SingleEndon", 0x12D3F70: "IW_Notify", 0x12D41E0: "Wait2",
    0x12D43E0: "WaitTillFrameEnd", 0x12D4510: "WaitTillMatch2", 0x12D4870: "IW_SingleWaitTill",
    0x12D48A0: "Wait", 0x12E9230: "Nop",
}
WAITS = {"Wait": "wait", "Wait2": "wait_other", "WaitTillFrameEnd": "waittillframeend",
         "IW_SingleWaitTill": "waittill", "WaitTillMatch2": "waittillmatch"}
THREAD_CALLS = {"ScriptThreadCall": "thread", "ScriptThreadCallPointer": "thread",
                "ScriptMethodThreadCall": "method_thread",
                "ScriptMethodThreadCallPointer": "method_thread",
                "ClassFunctionThreadCall": "method_thread"}
OBJECT_PUB = ("levelId", "gameId", "animId", "worldId", "classesId")
GDB_MAGIC = b"\x80GDB\r\n\x00"


def parse_gdb(d):
    """The official linker's per-script debug file (<path>.gsc.gdb), as far as measured:
    magic 80 'GDB' 0d 0a 00 <version 0x13>; u32 0; u32 source crc (the .gscc header's);
    {u32 offset, u32 count} for the line table, a second table (empty so far) and the string
    table. Line table entry i (8 bytes, u32 code offset, u32 0) is where line i+1 starts.
    Returns {"crc", "line_starts", "strings"}."""
    if d[:7] != GDB_MAGIC:
        raise ValueError("not a T7 .gdb file")
    crc, loff, lcount, _toff, _tcount, soff, scount = struct.unpack_from("<I6I", d, 12)
    starts = [struct.unpack_from("<I", d, loff + 8 * i)[0] for i in range(lcount)]
    strings, o = [], soff
    for _ in range(scount):
        e = d.index(b"\0", o)
        strings.append(d[o:e].decode("latin-1"))
        o = e + 1
    return {"version": d[7], "crc": crc, "line_starts": starts, "strings": strings}


class _Thread:
    __slots__ = ("name", "pending", "wait", "stack", "last_pc", "last_op", "error", "created")

    def __init__(self, name, pending=None):
        self.name = name
        self.pending = pending   # thread_create fields, emitted at the first opcode
        self.wait = None         # the wait_start record while the thread waits
        self.stack = []          # script function names (calls)
        self.last_pc = 0
        self.last_op = None
        self.error = None
        self.created = pending is not None


class Tracer:
    """Hooks a booted T7VM (after track_threads/trace_errors) and fills `trace` (trace.Trace).
    `names`: {t7 canon hash: word} for event, function and namespace names; `builtins`:
    {handler rva: name}; `calls` adds call/return; `ops` caps the op records (0: none)."""

    def __init__(self, vm, trace, names, builtins=None, calls=False, ops=0):
        self.vm = vm
        self.tr = trace
        self.names = names
        self.builtins = builtins or {}
        self.calls = calls
        self.ops = ops
        self.smap = SourceMap()
        self.threads = {}        # thread variable id -> _Thread
        self.spawn = None        # thread call being dispatched: fields for the new thread
        self.notify = None       # the notify record whose handler is running
        self.last_notify = {}    # (object id, event hash) -> seq
        self.fs = vm.B + VM_FS + 0x20 * vm.inst
        for rva, rec in DISPATCH.items():
            vm.watch(rva, (lambda r: lambda v: self._dispatch(r))(rec))
        vm.watch(THREAD_REMOVE, lambda v: self._thread_remove(v.emu.arg(1) & 0xFFFFFFFF))
        vm.on_thread_alloc.append(self._thread_alloc)
        vm.on_error.append(self._error)

    # --- scripts ---------------------------------------------------------------------------
    def add_script(self, vpath, data, gdb=None, file=None):
        """Map a script added with vm.add_script: exports (gscc header) and the line table."""
        base, size = self.vm.script_buffers[vpath.lower()]
        words = sorted(set(self.names.values()))
        tables = _read_tables()(data, words)
        funcs = [(e["address"], "%s::%s" % (e["namespace"], e["name"])) for e in tables["exports"]]
        lines = None
        extra = {"crc": tables["source_crc"]}
        if gdb:
            g = parse_gdb(gdb)
            if "0x%08x" % g["crc"] == tables["source_crc"]:
                lines = g["line_starts"]
            else:
                extra["gdb"] = "crc differs (%#x)" % g["crc"]
        src = self.tr.add_source(vpath, file=file, lines=bool(lines), **extra)
        self.smap.add(base, size, src, funcs, lines)
        return src

    # --- values ----------------------------------------------------------------------------
    def obj(self, oid):
        vm = self.vm
        for k in OBJECT_PUB:
            if oid == vm.pub(vm.inst, k):
                return k[:-2]
        _, t, _ = vm.var(vm.inst, oid)
        return "%s#%d" % (VT_NAMES.get(t & 0x1F, "obj"), oid)

    def hash_name(self, h):
        return self.names.get(h, "#%08x" % h)

    def value(self, va):
        """A VM value {u64, u32 type} as GSC-like display text."""
        vm = self.vm
        v, t = struct.unpack("<QI", bytes(vm.uc.mem_read(va, 12)))
        t &= 0x1F
        if t == VT["pointer"]:
            return self.obj(v & 0xFFFFFFFF)
        if t in (VT["string"], VT["istring"]):
            s = vm.sl_string(v & 0xFFFFFFFF)
            return '"%s"' % (s[:200] if s is not None else "?")
        if t == VT["hash"]:
            return '#"%s"' % self.hash_name(v & 0xFFFFFFFF)
        if t == VT["function"]:
            return "&" + (self.smap.loc(v) or {}).get("func", "%#x" % v)
        if t == VT["int"]:  # the low 32 bits (GetZero leaves the upper half as it was)
            return str(struct.unpack("<i", struct.pack("<I", v & 0xFFFFFFFF))[0])
        if t in (VT["undefined"], VT["float"], VT["vector"]):
            return vm.value_text(va)
        return "<%s>" % VT_NAMES.get(t, "type%d" % t)

    def event_name(self, va):
        v, t = struct.unpack("<QI", bytes(self.vm.uc.mem_read(va, 12)))
        t &= 0x1F
        if t == VT["hash"]:
            return v & 0xFFFFFFFF, self.hash_name(v & 0xFFFFFFFF)
        if t in (VT["string"], VT["istring"]):
            return None, self.vm.sl_string(v & 0xFFFFFFFF)
        return None, self.value(va)

    def obj_at(self, va):
        v, t = struct.unpack("<QI", bytes(self.vm.uc.mem_read(va, 12)))
        if t & 0x1F == VT["pointer"]:
            return v & 0xFFFFFFFF, self.obj(v & 0xFFFFFFFF)
        return None, self.value(va)

    def args_below(self, va, limit=16):
        """Values from va down to the PreScriptCall marker (type precodepos)."""
        out = []
        for _ in range(limit):
            t = struct.unpack("<I", bytes(self.vm.uc.mem_read(va + 8, 4)))[0] & 0x1F
            if t == VT["precodepos"]:
                break
            out.append(self.value(va))
            va -= 0x10
        return out

    # --- hooks -----------------------------------------------------------------------------
    def now(self):
        return self.vm.now()

    def state(self, tid):
        st = self.threads.get(tid)
        if st is None:  # a thread that existed before tracing started
            st = self.threads[tid] = _Thread(self.vm.thread_name(tid))
        return st

    def _thread_alloc(self, tid, name):
        pend = self.spawn or {"kind": "exec", "parent": None}
        self.spawn = None
        pend["id"] = tid
        self.threads[tid] = _Thread(name, pend)

    def _thread_remove(self, tid):
        st = self.threads.get(tid)
        if st is None or not st.created:
            return  # an endon record or a call frame, not a thread
        by = obj = event = None
        if st.last_op in ("End", "Return") and not st.stack:
            reason = "return"
        elif self.notify is not None and self.notify.get("th") != st.name:
            reason, by = "endon", self.notify["seq"]
            obj, event = self.notify.get("obj"), self.notify.get("event")
        elif st.error:
            reason = "error"
        else:
            reason = "killed"
        if st.wait:
            st.wait = None
        self.tr.emit("thread_end", self.now(), th=st.name, reason=reason, by=by, obj=obj,
                     event=event, loc=self.smap.loc(st.last_pc))
        del self.threads[tid]

    def _error(self, msg, terminal):
        vm = self.vm
        tid = vm.current_thread()
        st = self.state(tid)
        pc_now = vm.r64(self.fs) - 2
        loc = self.smap.loc(st.last_pc)
        extra = {}
        if msg.lower().startswith("invalid opcode"):
            pos = pc_now + 1 & ~1
            extra = {"at": self.smap.loc(pos),
                     "opcode": "%#06x" % struct.unpack("<H", bytes(vm.uc.mem_read(pos, 2)))[0]}
        st.error = msg
        self.tr.emit("error", self.now(), th=st.name, msg=msg, terminal=terminal, loc=loc, **extra)

    def _dispatch(self, recovery):
        vm = self.vm
        pc = vm.r64(self.fs) - 2
        top = vm.r64(self.fs + 8)
        opv = struct.unpack("<H", bytes(vm.uc.mem_read(pc, 2)))[0]
        idx = opv & ~0x2000
        h = vm.r64(vm.B + OPTAB[bool(opv & 0x2000)] + 8 * idx) - vm.B if idx < 0x2000 else 0
        op = HANDLERS.get(h, "op_%04x" % opv)
        tid = vm.current_thread()
        st = self.state(tid)
        t = self.now()
        self.notify = None
        self.spawn = None
        tr = self.tr
        if st.pending is not None:
            p, st.pending = st.pending, None
            entry = self.smap.function_at(pc) or (self.smap.loc(pc) or {}).get("func")
            tr.emit("thread_create", t, th=st.name, id=p.get("id"), parent=p.get("parent"),
                    kind=p.get("kind"), entry=entry, self=p.get("self"), loc=p.get("loc"),
                    entry_loc=self.smap.loc(pc))
        elif st.wait is not None:
            w, st.wait = st.wait, None
            by = None
            if w.get("_key"):
                by = self.last_notify.get(w["_key"])
            tr.emit("wait_end", t, th=st.name, kind=w["kind"], since=w["t"], wait=w["seq"],
                    by=by, loc=self.smap.loc(pc))
        loc = None
        f = self.smap.function_at(pc)
        if f is not None and not recovery:
            if self.calls:
                loc = self.smap.loc(pc)
                tr.emit("call", t, th=st.name, func=f, loc=loc,
                        **{"from": self.smap.loc(st.last_pc) if st.stack else None})
            st.stack.append(f)
        if self.ops:
            loc = loc or self.smap.loc(pc)
            tr.emit("op", t, th=st.name, op=op, loc=loc, recovery=True if recovery else None)
        if recovery:
            st.last_pc, st.last_op = pc, op
            return
        if op in ("End", "Return"):
            func = st.stack.pop() if st.stack else None
            if self.calls:
                tr.emit("return", t, th=st.name, func=func, loc=loc or self.smap.loc(pc))
        elif op in WAITS:
            self._wait(st, op, pc, top, t)
        elif op == "IW_Notify":
            oid, o = self.obj_at(top)
            eh, ev = self.event_name(top - 0x10)
            rec = tr.emit("notify", t, th=st.name, obj=o, event=ev,
                          args=self.args_below(top - 0x20), loc=self.smap.loc(pc))
            self.notify = rec
            if oid is not None and eh is not None:
                self.last_notify[(oid, eh)] = rec["seq"]
        elif op == "IW_SingleEndon":
            _, o = self.obj_at(top)
            _, ev = self.event_name(top - 0x10)
            tr.emit("endon", t, th=st.name, obj=o, event=ev, loc=self.smap.loc(pc))
        elif op in ("CallBuiltinFunction", "CallBuiltinMethod"):
            pos = pc + 2
            n = vm.uc.mem_read(pos, 1)[0]
            fp = vm.r64((pos + 1 + 7) & ~7)
            name = self.builtins.get(fp - vm.B, "%#x" % (fp - vm.B))
            if isinstance(name, int):  # unnamed in gruff-data: the script's own words, or #hash
                name = self.hash_name(name)
            method = op == "CallBuiltinMethod"
            if method:
                self_ = self.value(top)
                args = [self.value(top - 0x10 * (i + 1)) for i in range(n)]
            else:
                self_ = None
                args = [self.value(top - 0x10 * i) for i in range(n)]
            tr.emit("builtin", t, th=st.name, name=name, method=method or None, self=self_,
                    args=args, loc=self.smap.loc(pc))
        elif op in THREAD_CALLS:
            kind = THREAD_CALLS[op]
            self.spawn = {"kind": kind, "parent": st.name, "loc": self.smap.loc(pc)}
            if kind == "method_thread":
                self.spawn["self"] = self.value(top)
        st.last_pc, st.last_op = pc, op

    def _wait(self, st, op, pc, top, t):
        kind = WAITS[op]
        kw = {}
        key = None
        if kind in ("wait", "wait_other"):
            kw["duration"] = self.value(top)
        elif kind in ("waittill", "waittillmatch"):
            oid, kw["obj"] = self.obj_at(top)
            eh, kw["event"] = self.event_name(top - 0x10)
            if kind == "waittillmatch":
                n = self.vm.uc.mem_read(pc + 2, 1)[0]  # operand byte: number of match values
                kw["match"] = [self.value(top - 0x10 * (i + 2)) for i in range(n)]
            if oid is not None and eh is not None:
                key = (oid, eh)
        rec = self.tr.emit("wait_start", t, th=st.name, kind=kind, loc=self.smap.loc(pc), **kw)
        st.wait = {"kind": kind, "t": t, "seq": rec["seq"] if rec else None, "_key": key}


def _read_tables():
    """tools/linker/gscc_imports.read_tables (the .gscc header reader), loaded by path."""
    spec = importlib.util.spec_from_file_location("gscc_imports", GSCC_TABLES)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.read_tables


def builtin_names(path):
    """{handler rva: name, or the canon hash when gruff-data has no name} from
    gruff-data/builtins/t7.toml (functions and methods)."""
    import tomllib
    with open(path, "rb") as f:
        d = tomllib.load(f)
    out = {}
    for b in d.get("builtin", []):
        if b.get("handler_rva") and b.get("vm") == "gsc":
            out.setdefault(int(b["handler_rva"], 16), b.get("name") or int(b["hash"], 16))
    return out

