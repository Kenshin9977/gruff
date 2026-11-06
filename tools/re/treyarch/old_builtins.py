"""Extract the builtin catalogue of the 32-bit Treyarch games (T4 World at War, T5 Black Ops, T6
Black Ops II) from their unwrapped executables, statically and under Unicorn. The game is never
run.

    py -3.11 -I treyarch/old_builtins.py extract --game t4|t5|t5mp|t6|t6zm|t6sp [--exe X] [--out Y]

What these engines look like (see README_t4.md, README_t5.md, README_t6.md):

- The tables are static data: arrays of {char *name, handler, int developer} (T4/T5, 12 bytes)
  or {char *name, u32 id, u32 min_args, u32 max_args, handler, u32 developer} (T6, 0x18 bytes).
  Lookups compare the name string (inline strcmp loop); there is no hash.
- T4/T5 compile scripts in the executable: the compiler's call emitters (function call, method
  call) branch on the script instance (0 = GSC, 1 = CSC) and call the lookups. T6 links compiled
  scripts: the import linker branches on the import kind (2 = function call, patched to opcode
  0x29; 4 = method call, opcode 0x2a) and on the instance.

Steps (each fact measured twice where the engine allows it):

1. Roles: each role's code path (emitter or linker case) is emulated from its instance branch for
   inst 0 and 1, calls skipped and recorded: the lookups called, in order. Anchors are checked
   (the developer-call error string must be referenced right after the path).
2. Tables: each lookup is emulated with an absent name; its 4-byte reads at the entry stride give
   each table's address and entry count, in order. Measure 2: the scan loop's static bound
   (`cmp esi, N*stride`).
3. Round trip: every entry is looked up again by name through the role's lookups; the handler
   (and developer flag, min, max where the lookup returns them) must be the entry's. A name found
   earlier in the role is the later entry's shadow (kept, marked).
4. Handlers: each handler runs with N parameters on a fake VM stack (scrVmPub's top and
   outparamcount addresses read off Scr_GetInt's own bound check). The real accessors run; Scr_Error
   is hooked and *returns* (continue mode), so a handler goes on past a type or object error: the
   messages give the type of each parameter ("type undefined is not an int": the last stack slot
   read) and the missing ones ("parameter %d does not exist"). T4/T5 have no arity in the table:
   min/max come from these runs (the rule is in arity_from_runs).
"""
import argparse
import json
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common.image import Image  # noqa: E402
from common.emu import Emu, unicorn, X  # noqa: E402
from common import paths  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(os.path.dirname(HERE), "out")
INST = "INST"
DEV_T45 = "return value of developer command can not be accessed if not in a /# ... #/ comment"
DEV_T6 = "**** Dev only calls must be wrapped in a devblock /#...#/ : "

# Per-game anchors: each is a hypothesis checked at run time.
#   roles: kind -> (start, join, regs): the role's path from its instance branch (start) to the
#   point where both instance paths join; regs set at start (INST = the instance).
PROFILES = {
    "t4": {
        "layout": "old12", "dev_string": DEV_T45,
        "what": "CoDWaW LanFixed.exe (unwrapped SP build of the Steam CoDWaW.exe 1.7), flattened",
        "roles": {"function": (0x682D88, 0x682DC8, {"esi": INST, "ecx": 0}),
                  "method": (0x68302F, 0x683064, {"ebx": INST})},
    },
    "t5": {
        "layout": "old12", "dev_string": DEV_T45,
        "what": "BlackOps.exe (Steam, SP and zombies), flattened",
        "roles": {"function": (0x8A02ED, 0x8A0311, {"esi": INST, "ebp": 0}),
                  "method": (0x8A0520, 0x8A0544, {"esi": INST, "ebp": 0})},
    },
    "t5mp": {
        "layout": "old12", "dev_string": DEV_T45, "what": "BlackOpsMP.exe (Steam, file layout)",
        "roles": None,  # filled by find_emitters (same code shape as t5)
    },
    "t6": {
        "layout": "t6", "dev_string": DEV_T6,
        "what": "t6mp.exe unpacked, flattened",
        "roles": {"function": (0x7E094A, 0x7E0982, {"ebp": INST}),
                  "method": (0x7E0AB8, 0x7E0AF0, {"ebp": INST})},
    },
    "t6zm": {"layout": "t6", "dev_string": DEV_T6,
             "what": "t6zm.exe unpacked (file layout)", "roles": None},
    "t6sp": {"layout": "t6", "dev_string": DEV_T6,
             "what": "t6sp.exe unpacked (file layout)", "roles": None},
}
INSTS = {0: "gsc", 1: "csc"}
STRIDE = {"old12": 12, "t6": 0x18}
VARIADIC = 0x7FFFFFFF
IDENT = re.compile(r"^[a-z_][a-z0-9_]*$")
REG = {n: getattr(X, "UC_X86_REG_" + n.upper()) for n in
       ("eax", "ebx", "ecx", "edx", "esi", "edi", "ebp")}


def die(msg):
    raise SystemExit("old_builtins: " + msg)


def head(img, va):
    """Start of the x86 function holding va: the 16-aligned address after int3 padding."""
    o = va - img.base
    while o > 0:
        if o % 16 == 0 and img.d[o - 1] == 0xCC:
            return img.base + o
        o -= 1
    die("no function head before %#x" % va)


# ---------------------------------------------------------------------------------------------
# entries
def read_entry(img, layout, va, mem=None):
    rd = (lambda a: struct.unpack("<I", bytes(mem(a, 4)))[0]) if mem else img.u32
    np_ = rd(va)
    e = {"va": va, "name_ptr": np_, "name": img.cstr(np_, 80) if img.in_image(np_) else None}
    if layout == "old12":
        e.update(handler=rd(va + 4), dev=rd(va + 8))
    else:
        e.update(id=rd(va + 4), min=rd(va + 8), max=rd(va + 12), handler=rd(va + 16),
                 dev=rd(va + 20))
    return e


def shaped(img, layout, e):
    ok = bool(e["name"]) and IDENT.match(e["name"].lower()) is not None \
        and img.is_code(e["handler"]) and e["dev"] < 0x100
    if layout == "t6":
        ok = ok and (e["min"] <= e["max"] <= 0x100 or (e["min"] <= 0x100 and e["max"] == VARIADIC)
                     or e["min"] == e["max"] == 0xFFFFFFFF)
    return ok


# ---------------------------------------------------------------------------------------------
# 1. roles
def find_emitters(img, prof, log):
    """For a sibling build (MP, zm, sp) of a profiled game: the role paths, found as in the
    profiled exe: the instance branch before the developer-call check of each emitter / linker
    case. T4/T5: `cmp esi, ebp; jne` with two lookup calls; T6: `test ebp, ebp; jne`."""
    devs = img.find_all(prof["dev_string"].encode() + b"\0")
    if len(devs) != 1:
        die("developer string found %d times" % len(devs))
    refs = img.code_refs(devs[0])
    roles = {}
    if prof["layout"] == "old12":
        # each emitter: ... cmp esi, ebp; jne B; (lea, push, lea, push, call L0; jmp J);
        # B: (lea, push, lea, push, call L1); J: ...  The emitter with the lower address is the
        # function emitter in T5 SP (checked on the result: the method lookups are composites).
        found = []
        for r in sorted(refs):
            h = head(img, r)
            ins = img.disasm(h, r - h)
            for k, i in enumerate(ins):
                if i.mnemonic == "cmp" and i.op_str == "esi, ebp" and \
                        ins[k + 1].mnemonic == "jne":
                    calls = [j for j in ins[k:k + 14] if j.mnemonic == "call"]
                    jmps = [j for j in ins[k + 2:k + 14] if j.mnemonic == "jmp"]
                    if len(calls) >= 2 and jmps:
                        found.append((i.address, int(jmps[0].op_str, 16)))
                        break
        if len(found) != 2:
            die("emitters: %d instance branches found" % len(found))
        roles = {"function": (found[0][0], found[0][1], {"esi": INST, "ebp": 0}),
                 "method": (found[1][0], found[1][1], {"esi": INST, "ebp": 0})}
    else:
        found = []
        for r in sorted(refs):
            # the linker holds a jump table: decode from the `test ebp, ebp; jne` bytes
            o = img.d.rfind(bytes([0x85, 0xED, 0x75]), r - img.base - 0x80, r - img.base)
            if o < 0:
                continue
            ins = img.disasm(img.base + o, r - img.base - o)
            for k, i in enumerate(ins):
                if i.mnemonic == "test" and i.op_str == "ebp, ebp" and \
                        ins[k + 1].mnemonic == "jne":
                    jmps = [j for j in ins[k + 2:k + 14] if j.mnemonic == "jmp"]
                    if jmps:
                        found.append((i.address, int(jmps[0].op_str, 16)))
                        break
        found = sorted(set(found))
        if len(found) != 2:
            die("linker cases: %d instance branches found" % len(found))
        roles = {"function": (found[0][0], found[0][1], {"ebp": INST}),
                 "method": (found[1][0], found[1][1], {"ebp": INST})}
    log.append("role paths found: %s" % {k: (hex(a), hex(b)) for k, (a, b, _) in roles.items()})
    return roles


def check_role_anchor(img, prof, kind, start, join):
    """The developer-call error must be referenced right after the path (it is the check on the
    lookup's developer result); T6: the case must patch its opcode (0x29 call, 0x2a method)."""
    devs = img.find_all(prof["dev_string"].encode() + b"\0")
    refs = img.code_refs(devs[0])
    if not any(join < r < join + 0x120 for r in refs):
        die("%s path %#x..%#x: developer-call message not referenced after it" % (kind, start, join))
    if prof["layout"] == "t6":
        want = {"function": 0x29, "method": 0x2A}[kind]
        ins = img.disasm(join, 0x180)
        pat = [i for i in ins if i.mnemonic == "mov" and i.op_str.startswith("byte ptr [esi + e")
               and i.op_str.endswith(", 0x%x" % want)]
        if not pat:
            die("%s linker case: no opcode 0x%x patch after %#x" % (kind, want, join))
        return "patches opcode 0x%x at %#x" % (want, pat[0].address)
    return "developer-call check follows"


def run_role(img, start, join, regs, inst):
    """Emulate a role path from its instance branch to the join; record the calls (skipped,
    eax = 0: not found)."""
    emu = Emu(img, null_page=True)
    emu.map_on_unmapped()
    uc = emu.uc
    called = []
    sp = emu.stack + 0x100000
    uc.reg_write(X.UC_X86_REG_ESP, sp)
    for r, v in regs.items():
        uc.reg_write(REG[r], inst if v == INST else v)

    def on_code(uc, addr, size, ud):
        if addr == join:
            uc.emu_stop()
            return
        ins = next(img.md.disasm(bytes(uc.mem_read(addr, 16)), addr))
        if ins.mnemonic == "call":
            called.append(int(ins.op_str, 16) if ins.op_str.startswith("0x") else None)
            uc.reg_write(X.UC_X86_REG_EAX, 0)
            uc.reg_write(X.UC_X86_REG_EIP, addr + ins.size)

    uc.hook_add(unicorn.UC_HOOK_CODE, on_code, begin=start, end=join)
    try:
        uc.emu_start(start, 0, count=500)
    except unicorn.UcError as ex:
        die("role path %#x inst %d: %s at %#x" % (start, inst, ex, emu.ip()))
    if emu.ip() != join:
        die("role path %#x inst %d did not reach the join %#x (at %#x)" % (start, inst, join,
                                                                           emu.ip()))
    if not called or None in called:
        die("role path %#x inst %d: calls %s" % (start, inst, called))
    return called


# ---------------------------------------------------------------------------------------------
# 2. tables: lookup traced with an absent name; static loop bound
class Lookup:
    """Lookups are cdecl (const char **pName, int *pType[, int *pMin, int *pMax]); some T4
    composites take pName in esi and pType in edi: both are set."""

    def __init__(self, img):
        self.img = img
        self.emu = Emu(img, null_page=True)
        self.emu.map_on_unmapped()
        self.buf = self.emu.alloc(0x400)

    def call(self, va, name, reads=None):
        emu, uc = self.emu, self.emu.uc
        s = self.buf + 0x100
        uc.mem_write(s, name.encode() + b"\0")
        uc.mem_write(self.buf, struct.pack("<IIII", s, 0xEEEEEEEE, 0xEEEEEEEE, 0xEEEEEEEE))
        pn, pt, pmin, pmax = self.buf, self.buf + 4, self.buf + 8, self.buf + 12
        h = None
        if reads is not None:
            img = self.img

            def hr(uc, acc, addr, size, val, ud):
                if size == 4 and img.base <= addr < img.base + img.size:
                    reads.append((addr, uc.reg_read(X.UC_X86_REG_EIP)))
            h = uc.hook_add(unicorn.UC_HOOK_MEM_READ, hr)
        r, err = emu.run(va, (pn, pt, pmin, pmax), count=5_000_000,
                         regs={X.UC_X86_REG_ESI: pn, X.UC_X86_REG_EDI: pt,
                               X.UC_X86_REG_ECX: pn, X.UC_X86_REG_EBX: pn})
        if h is not None:
            uc.hook_del(h)
        if err:
            die("lookup %#x(%r): %s" % (va, name, err))
        out = struct.unpack("<IIII", bytes(uc.mem_read(self.buf, 16)))
        return r, {"name_ptr": out[0], "dev": out[1], "min": out[2], "max": out[3]}


def trace_tables(img, lk, va, layout):
    reads = []
    r, _ = lk.call(va, "zz_absent_builtin_name", reads)
    if r:
        die("lookup %#x returned %#x for an absent name" % (va, r))
    st = STRIDE[layout]
    # a table is a run of reads at the stride made by one instruction (adjacent tables scanned by
    # different loops are told apart by the reading instruction)
    order, seen = [], {}
    for a, pc in reads:
        if a not in seen:
            seen[a] = pc
            order.append(a)
    tables = []
    for a in order:
        pc = seen[a]
        if seen.get(a - st) == pc:
            continue
        n = 1
        while seen.get(a + st * n) == pc:
            n += 1
        ents = [read_entry(img, layout, a + st * k) for k in range(n)]
        if not all(shaped(img, layout, e) for e in ents):
            if n > 1:
                die("lookup %#x reads a %d-entry run at %#x that is not a builtin table" % (va, n, a))
            continue
        tables.append((a, ents))
    return tables


def static_bound(img, table, stride):
    """`mov ecx, [esi + table]` ... `add esi, stride` ... `cmp esi, N`: N / stride."""
    out = set()
    for ref in img.code_refs(table):
        for i in img.disasm(ref, 0x60):
            if i.mnemonic == "cmp" and re.match(r"^e\w\w, (0x[0-9a-f]+|\d+)$", i.op_str):
                n = int(i.op_str.split(", ")[1], 0)
                if n % stride == 0:
                    out.add(n // stride)
                break
            if i.mnemonic == "ret":
                break
    return sorted(out)


# ---------------------------------------------------------------------------------------------
# 4. handlers
def find_vm(img, log):
    """Scr_GetInt (it loads "type %s is not an int"): its bound check gives scrVmPub's
    outparamcount and top addresses and the instance stride; its error path gives Scr_Error
    (the call after the formatter of "parameter %d does not exist") and where the message is."""
    s = img.find_all(b"type %s is not an int\0")
    if len(s) != 1:
        die("'type %%s is not an int' found %d times" % len(s))
    refs = img.code_refs(s[0])
    if len(refs) != 1:
        die("Scr_GetInt: %d references to its error" % len(refs))
    gi = head(img, refs[0])
    ins = img.disasm(gi, 0x120)
    stride = outp = top = None
    for i in ins:
        m = re.match(r"^imul (e\w\w), \1, (0x[0-9a-f]+)$", i.op_str and i.mnemonic + " " + i.op_str)
        if m and stride is None:
            stride = int(m.group(2), 16)
        if i.mnemonic == "cmp" and outp is None:
            m = re.match(r"^e\w\w, dword ptr \[e\w\w \+ (0x[0-9a-f]+)\]$", i.op_str)
            if m:
                outp = int(m.group(1), 16)
        if i.mnemonic == "mov" and outp is not None and top is None:
            m = re.match(r"^e\w\w, dword ptr \[e\w\w \+ (0x[0-9a-f]+)\]$", i.op_str)
            if m:
                top = int(m.group(1), 16)
    if not (stride and outp and top and 0 < outp - top < 0x40):
        die("Scr_GetInt %#x: bound check not decoded (stride %s, outparamcount %s, top %s)"
            % (gi, stride, outp, top))
    miss = img.find_all(b"parameter %d does not exist\0")[0]
    # VariableValue layout: the type check `cmp dword ptr [eax + 4], N` (T4/T5: {u, type}) or
    # `cmp dword ptr [eax], N` (T6: {type, u})
    type_off = None
    for i in ins:
        if i.mnemonic == "cmp" and re.match(r"^dword ptr \[eax( \+ 4)?\], (0x)?[0-9a-f]+$",
                                           i.op_str):
            type_off = 4 if "+ 4" in i.op_str else 0
            break
    if type_off is None:
        die("Scr_GetInt %#x: type check not decoded" % gi)
    # Scr_Error: the call after the formatter of Scr_GetInt's type error (T6 inlines Scr_Error
    # in its missing-parameter path)
    err = msg_at = None
    for k, i in enumerate(ins):
        if i.mnemonic == "push" and img.operand_addr(i) == s[0]:
            calls = [j for j in ins[k:k + 12] if j.mnemonic == "call"]
            err = int(calls[1].op_str, 16)
            before = ins[ins.index(calls[1]) - 1]
            if before.mnemonic == "mov" and before.op_str == "ecx, eax":
                msg_at = ("ecx",)
            else:  # push 0; push eax (message); push inst; call
                pushes = [j for j in ins[ins.index(calls[0]):ins.index(calls[1])]
                          if j.mnemonic == "push"]
                if [p.op_str for p in pushes][-2] != "eax":
                    die("Scr_Error arguments not decoded at %#x" % calls[1].address)
                msg_at = ("arg", 1)
            break
    if err is None:
        die("Scr_Error not found in Scr_GetInt's error path")
    # Scr_Error: `imul r, r, S; cmp dword ptr [r + G], 0` (the error message slot, set once),
    # then its last call / tail jump is the runtime error (some paths call it directly)
    eb = img.body(err)
    gmsg = gstride = internal = None
    for i in eb:
        m = re.match(r"^e\w\w, e\w\w, (0x[0-9a-f]+)$", i.op_str)
        if i.mnemonic == "imul" and m and gstride is None:
            gstride = int(m.group(1), 16)
        m = re.match(r"^dword ptr \[e\w\w \+ (0x[0-9a-f]+)\], 0$", i.op_str)
        if i.mnemonic == "cmp" and m and gmsg is None:
            gmsg = int(m.group(1), 16)
        if i.mnemonic in ("call", "jmp") and i.op_str.startswith("0x") and \
                not err <= int(i.op_str, 16) < err + 0x100:
            internal = int(i.op_str, 16)
    if not (gmsg and gstride and internal):
        die("Scr_Error %#x: message slot / runtime error not decoded" % err)
    # accessor bodies (functions that load a parameter error), to tell Scr_GetNumParam reads
    acc_ranges = []
    for pat in (rb"type %s is not an? [a-z ]+\0", rb"cannot cast %s to [a-z]+\0",
                rb"parameter %d does not exist\0"):
        for m in re.finditer(pat, img.d):
            for r in img.code_refs(img.base + m.start()):
                h = head(img, r)
                acc_ranges.append((h, img.end_of(h)))
    va_ = None
    for k, i in enumerate(ins):
        if i.mnemonic == "push" and img.operand_addr(i) == miss:
            va_ = int([j for j in ins[k:k + 6] if j.mnemonic == "call"][0].op_str, 16)
            break
    # the type name table the error indexes: `mov eax, dword ptr [edx*4 + T]`
    typenames = None
    for i in ins:
        m = re.match(r"^e\w\w, dword ptr \[e\w\w\*4 \+ (0x[0-9a-f]+)\]$", i.op_str)
        if i.mnemonic == "mov" and m:
            t = int(m.group(1), 16)
            typenames = [img.cstr(img.u32(t + 4 * k), 40) for k in range(24)]
            break
    if not typenames or typenames[:3] != ["undefined", "object", "string"]:
        die("Scr_GetInt: type name table not found (%s)" % typenames)
    vm_types = {}
    for lab, tn in (("int", "int"), ("float", "float"), ("string", "string"),
                    ("istring", "localized string"), ("vector", "vector"),
                    ("entity", "object"), ("object", "object"), ("function", "function"),
                    ("anim", "animation"), ("bool", "int")):
        if tn in typenames:
            vm_types[lab] = typenames.index(tn)
    # _vsnprintf: va's call `push ap; push fmt; push 0x400; push buf; call X`
    vsn = None
    vb = img.disasm(va_, 0x80)
    for k, i in enumerate(vb):
        if i.mnemonic == "push" and i.op_str == "0x400" and vb[k + 2].mnemonic == "call":
            vsn = int(vb[k + 2].op_str, 16)
            break
    if vsn is None:
        die("va %#x: the _vsnprintf call not decoded" % va_)
    vm = {"type_off": type_off, "vsnprintf": vsn, "typenames": typenames, "type_ids": vm_types, "internal_error": internal, "error_msg_slot": gmsg, "error_msg_stride": gstride,
          "va": va_, "getint": gi, "stride": stride, "outparamcount": outp, "top": top, "scr_error": err,
          "msg_at": msg_at, "acc_ranges": sorted(set(acc_ranges))}
    log.append("Scr_GetInt %#x: scrVmPub top %#x, outparamcount %#x, stride %#x; Scr_Error %#x "
               "(message in %s); %d accessor bodies" % (gi, top, outp, stride, err, msg_at,
                                                         len(vm["acc_ranges"])))
    return vm


def c_format(emu, fmt_va, first_arg, va_list=None):
    """printf-style formatting of a format string in emulated memory, arguments on the stack
    from argument `first_arg`, or (va_list given) at that address."""
    def cstr(p):
        try:
            return bytes(emu.uc.mem_read(p, 256)).split(bytes(1))[0].decode("latin1")
        except unicorn.UcError:
            return "?"
    fmt = cstr(fmt_va)
    k = [first_arg]

    def nxt():
        v = emu.arg32(k[0]) if va_list is None else emu.r32(va_list + 4 * k[0])
        k[0] += 1
        return v

    def sub(m):
        c = m.group(2)
        if c == "%":
            return "%"
        if c == "s":
            return cstr(nxt())
        if c in "di":
            v = nxt()
            return str(v - (1 << 32) if v >= 1 << 31 else v)
        if c == "c":
            return chr(nxt() & 0xFF)
        if c in "xXp":
            return "%x" % nxt()
        if c == "u":
            return str(nxt())
        if c in "fg":
            lo, hi = nxt(), nxt()
            return "%g" % struct.unpack("<d", struct.pack("<II", lo, hi))[0]
        return "?"
    return re.sub(r"%([-+ #0-9.]*l*)([%sdiuxXcfgp])", sub, fmt)


# kernel32 functions a handler may reach through the CRT: (return value, stdcall argument bytes)
IMPORT_STUBS = {"TlsGetValue": ("tls", 4), "GetLastError": (0, 0), "SetLastError": (0, 4),
                "EnterCriticalSection": (0, 4), "LeaveCriticalSection": (0, 4),
                "InterlockedIncrement": (1, 4), "InterlockedDecrement": (0, 4),
                "InterlockedExchange": (0, 8), "InterlockedCompareExchange": (0, 12),
                "GetTickCount": (0, 0), "timeGetTime": (0, 0), "Sleep": (0, 4),
                "GetCurrentThreadId": (1, 0), "QueryPerformanceCounter": (1, 4),
                "QueryPerformanceFrequency": (1, 4), "OutputDebugStringA": (0, 4),
                "RestoreLastError": (0, 4), "EncodePointer": ("arg0", 4),
                "DecodePointer": ("arg0", 4), "TlsSetValue": (1, 8),
                "GetCurrentProcessId": (1, 0), "GetCurrentThread": (1, 0)}


def imports(img):
    """{IAT slot va: function name} from the import directory."""
    d = img.d
    e = struct.unpack_from("<I", d, 0x3C)[0]
    rva = struct.unpack_from("<I", d, e + 24 + 104)[0]
    out = {}
    while rva:
        oft, _, _, name, ft = struct.unpack_from("<IIIII", d, rva)
        if not (oft or ft):
            break
        src = oft or ft
        k = 0
        while True:
            thunk = struct.unpack_from("<I", d, src + 4 * k)[0]
            if not thunk:
                break
            fn = ("#%d" % (thunk & 0xFFFF)) if thunk & 0x80000000 else \
                (img.cstr(img.base + thunk + 2, 80) if thunk < img.size else "?")
            out[img.base + ft + 4 * k] = fn
            k += 1
        rva += 20
    return out


def stub_imports(img, emu, runner):
    """Point every IAT slot at a stub; the listed ones return a neutral value, any other import
    stops the run (the handler left the game's code)."""
    uc = emu.uc
    page = emu.alloc(0x4000)
    names = {}
    for k, (slot, fn) in enumerate(sorted(imports(img).items())):
        a = page + 4 * k
        names[a] = fn
        uc.mem_write(a, bytes([0xC3]))
        uc.mem_write(slot, struct.pack("<I", a))

    def on_stub(uc, addr, size, ud):
        fn = names.get(addr)
        if fn in IMPORT_STUBS:
            rv, pop = IMPORT_STUBS[fn]
            emu.ret(runner.tls if rv == "tls" else emu.arg32(0) if rv == "arg0" else rv, pop)
        else:
            runner.import_stop = fn or "?"
            uc.emu_stop()
    uc.hook_add(unicorn.UC_HOOK_CODE, on_stub, begin=page, end=page + 0x4000)


TYPE_ERR = re.compile(r"^type (.+?) is not an? (.+)$")
CAST_ERR = re.compile(r"^cannot cast (.+?) to (\w+)$")
MISS_ERR = re.compile(r"^parameter (\d+) does not exist$")
LABELS = {"int": "int", "float": "float", "vector": "vector", "localized string": "istring",
          "function": "function", "entity": "entity", "object": "object", "anim": "anim",
          "animtree": "animtree", "string": "string", "bool": "bool", "hash": "hash"}


class Runner:
    """One handler, N parameters (all undefined) on a fake VM stack; real accessors; Scr_Error
    hooked to record its message and return (continue mode)."""

    def __init__(self, img, vm):
        self.img, self.vm = img, vm
        self.emu = Emu(img, null_page=True)
        self.emu.map_on_unmapped(cap=512)
        uc = self.emu.uc
        self.vstack = self.emu.alloc(0x10000)
        self.top = self.vstack + 0x8000
        uc.hook_add(unicorn.UC_HOOK_CODE, self._on_err, begin=vm["scr_error"], end=vm["scr_error"])
        uc.hook_add(unicorn.UC_HOOK_CODE, self._on_internal, begin=vm["internal_error"],
                    end=vm["internal_error"])
        uc.hook_add(unicorn.UC_HOOK_MEM_READ, self._on_slot, begin=self.vstack,
                    end=self.vstack + 0x10000)
        self.saved = {}
        self._own = {}
        self._heads = {}
        # the VM functions that write outparamcount (Scr_Add*, the out-parameter cleanup): a slot
        # read inside one of them (to its return) is the VM's, not a parameter read
        a = vm["outparamcount"]
        self.vm_internal = set()
        for r in img.code_refs(a):
            i = next(img.md.disasm(img.d[r - img.base:r - img.base + 16], r))
            if i.op_str.startswith("dword ptr [") and i.mnemonic in ("mov", "sub", "dec", "add",
                                                                     "inc"):
                self.vm_internal.add(head(img, r))
        for f in self.vm_internal:
            uc.hook_add(unicorn.UC_HOOK_CODE, self._on_vm_internal, begin=f, end=f)
        # every write outside the stacks is undone after the run (the image, the null page, the
        # heap, pages mapped on demand): runs are independent
        for lo, hi in ((img.base, img.base + img.size), (0, 0x100000),
                       (self.emu.heap0, self.emu.heap0 + 0x4000000)):
            uc.hook_add(unicorn.UC_HOOK_MEM_WRITE, self._on_write, begin=lo, end=hi - 1)
        self.no_restore = [(self.emu.stack, self.emu.stack + 0x200000),
                           (self.vstack, self.vstack + 0x10000)]
        self.watch = set()
        for inst in INSTS:
            a = vm["outparamcount"] + inst * vm["stride"]
            uc.hook_add(unicorn.UC_HOOK_MEM_READ, self._on_watch, begin=a, end=a + 3)
        self.fmtbuf = self.emu.alloc(0x1000)
        self.vec = self.emu.alloc(16)
        self.blank = self.emu.alloc(0x20000)  # what a fake self's every field points at
        uc.hook_add(unicorn.UC_HOOK_CODE, self._on_va, begin=vm["va"], end=vm["va"])
        uc.hook_add(unicorn.UC_HOOK_CODE, self._on_vsnprintf, begin=vm["vsnprintf"],
                    end=vm["vsnprintf"])
        # the CRT keeps FlsGetValue's address in a TLS slot and calls it to get its per-thread
        # data: TlsGetValue returns a stub `mov eax, ptd; ret 4` with a zeroed per-thread block
        self.tls = self.emu.alloc(0x1000)
        ptd = self.emu.alloc(0x1000)
        uc.mem_write(self.tls, bytes([0xB8]) + struct.pack("<I", ptd) + bytes([0xC2, 4, 0]))
        self.import_stop = None
        stub_imports(img, self.emu, self)

    def _on_write(self, uc, acc, addr, size, val, ud):
        for lo, hi in self.no_restore:
            if lo <= addr < hi:
                return
        for a in range(addr & ~3, addr + size, 4):
            if a not in self.saved:
                try:
                    self.saved[a] = bytes(uc.mem_read(a, 4))
                except unicorn.UcError:  # not mapped yet: mapped on demand, zeros
                    self.saved[a] = bytes(4)

    def _on_watch(self, uc, acc, addr, size, val, ud):
        if addr in self.watch:
            pc = uc.reg_read(X.UC_X86_REG_EIP)
            if not any(a <= pc < b for a, b in self.vm["acc_ranges"]):
                self.numparam_reads += 1

    def _on_vm_internal(self, uc, addr, size, ud):
        self.internal_sp.append(uc.reg_read(X.UC_X86_REG_ESP))

    def head_of(self, pc):
        if pc not in self._heads:
            try:
                self._heads[pc] = head(self.img, pc) if self.img.is_code(pc) else pc
            except SystemExit:
                self._heads[pc] = pc
        return self._heads[pc]

    def _on_va(self, uc, addr, size, ud):
        """va(fmt, ...): formatted in Python (the CRT's vsnprintf needs the OS's TLS)."""
        emu = self.emu
        out = c_format(emu, emu.arg32(0), 1)
        uc.mem_write(self.fmtbuf, out[:0xFFF].encode("latin1") + bytes(1))
        emu.ret(self.fmtbuf)

    def _on_vsnprintf(self, uc, addr, size, ud):
        """_vsnprintf(buf, count, fmt, va_list), formatted in Python (the CRT's needs the OS)."""
        emu = self.emu
        buf, n, fmt, ap = (emu.arg32(i) for i in range(4))
        out = c_format(emu, fmt, 0, ap).encode("latin1")[:max(0, n - 1)]
        try:
            uc.mem_write(buf, out + bytes(1))
        except unicorn.UcError:
            pass
        emu.ret(len(out))

    def own_ranges(self, handler):
        """Code whose slot reads are parameter reads: the accessors (functions raising a
        parameter error), the handler and its direct callees (Scr_GetDebugString-style readers).
        Not deeper: Scr_Add* -> the VM's out-parameter cleanup walks every slot."""
        if handler not in self._own:
            img = self.img
            r = [(handler, img.end_of(handler))]
            for t in img.direct_calls(handler):
                if img.is_code(t):
                    r.append((t, img.end_of(t)))
            self._own[handler] = r + self.vm["acc_ranges"]
        return self._own[handler]

    def _on_slot(self, uc, acc, addr, size, val, ud):
        if addr <= self.top + 7:
            i = (self.top + 7 - addr) // 8
            if 0 <= i < 64:
                sp = uc.reg_read(X.UC_X86_REG_ESP)
                self.internal_sp = [e for e in self.internal_sp if sp < e]
                if not self.internal_sp:  # not inside a VM function that changes outparamcount
                    self.reads.append(i)
                    self.read_pcs.append(uc.reg_read(X.UC_X86_REG_EIP))

    def _on_internal(self, uc, addr, size, ud):
        """The runtime error reached without Scr_Error (e.g. a failed string conversion): the
        message is in scrVarPub's error slot; cleared, and the error returns."""
        emu = self.emu
        m = "?"
        for inst in INSTS:
            slot = self.vm["error_msg_slot"] + inst * self.vm["error_msg_stride"]
            p = emu.r32(slot)
            if p:
                try:
                    m = bytes(uc.mem_read(p, 200)).split(bytes(1))[0].decode("latin1")
                except unicorn.UcError:
                    pass
                uc.mem_write(slot, bytes(4))
        self.errors.append((m, self.reads[-1] if self.reads else None))
        if len(self.errors) > 64:
            uc.emu_stop()
        emu.ret(0)

    def _on_err(self, uc, addr, size, ud):
        emu = self.emu
        if self.vm["msg_at"][0] == "ecx":
            p = uc.reg_read(X.UC_X86_REG_ECX)
        else:
            p = emu.arg32(self.vm["msg_at"][1])
        try:
            m = bytes(uc.mem_read(p, 200)).split(b"\0")[0].decode("latin1")
        except unicorn.UcError:
            m = "?"
        self.errors.append((m, self.reads[-1] if self.reads else None))
        if len(self.errors) > 64:
            uc.emu_stop()
        emu.ret(0)

    def run(self, handler, inst, nparams, slots=None, self_arrays=None, count=400_000,
            zero=False):
        """slots: {index: type label} given a value of that type (the others are undefined)."""
        uc, emu, vm = self.emu.uc, self.emu, self.vm
        uc.mem_write(self.vstack, bytes(0x10000))
        for i, lab in (slots or {}).items():
            tid = vm["type_ids"].get(lab)
            if tid is None:
                continue
            val = {"float": 0x3F800000, "vector": self.vec}.get(lab, 1)
            if zero and lab in ("int", "float", "bool"):
                val = 0  # second value: reads gated on a value (assert's message when false)
            pair = (val, tid) if vm["type_off"] == 4 else (tid, val)
            uc.mem_write(self.top - 8 * i, struct.pack("<II", *pair))
        filled = {}
        for base, n in (self_arrays or ()):  # fake self: entity 0 of each array the handler uses
            try:
                filled[base] = bytes(uc.mem_read(base, n))
                uc.mem_write(base, struct.pack("<I", self.blank) * (n // 4))
            except unicorn.UcError:
                filled.pop(base, None)
        top_va = vm["top"] + inst * vm["stride"]
        outp_va = vm["outparamcount"] + inst * vm["stride"]
        uc.mem_write(top_va, struct.pack("<I", self.top))
        uc.mem_write(outp_va, struct.pack("<I", nparams))
        self.watch = {outp_va}
        self.reads, self.errors, self.numparam_reads = [], [], 0
        self.read_pcs = []
        self.internal_sp = []
        self.import_stop = None
        self.arg_lo = 0
        emu.pages = 0
        # entref read: the handler's first stack argument slot
        sp_entry = emu.stack + 0x200000 - 0x1000 - 12 - 4
        got = {"arg": False}

        def on_arg(uc, acc, addr, size, val, ud):
            if acc == unicorn.UC_MEM_READ:
                got["arg"] = True
        ha = uc.hook_add(unicorn.UC_HOOK_MEM_READ, on_arg, begin=sp_entry + 4, end=sp_entry + 11)
        r, err = emu.run(handler, (0, 0, 0), count=count)
        uc.hook_del(ha)
        for a, b in self.saved.items():  # restore the image
            uc.mem_write(a, b)
        self.saved = {}
        for a, b in filled.items():
            uc.mem_write(a, b)
        types, missing, other = {}, False, []
        for m, last in self.errors:
            mm = MISS_ERR.match(m)
            if mm:
                if int(mm.group(1)) - 1 >= nparams:
                    missing = True
                continue  # else: the fall-through after a type error in the same accessor
            mt = TYPE_ERR.match(m) or CAST_ERR.match(m)
            if mt and mt.group(2) in LABELS and last is not None:
                types.setdefault(last, set()).add(LABELS[mt.group(2)])
                continue
            m2 = re.match(r"^(.+?) is not an? (entity|object|function pointer)$", m)
            if m2 and last is not None and m2.group(1) == "undefined":
                types.setdefault(last, set()).add(m2.group(2).split()[0])
                continue
            other.append(m)
        # a parameter read is one made by an accessor (a function that raises a parameter error)
        # or by the handler's own code; the VM's out-parameter cleanup also walks every slot
        # not a parameter read: the VM's out-parameter cleanup (Scr_Add* -> ClearOutParams) walks
        # every slot; it is the function that writes outparamcount (reads by it are dropped)
        reads = sorted(set(self.reads))
        return {"n": nparams, "reads": reads, "types": {i: sorted(v) for i, v in types.items()},
                "missing": missing, "errors": other[:8], "numparam_reads": self.numparam_reads,
                "arg_read": got["arg"],
                "end": ("import " + self.import_stop) if self.import_stop else
                (err.split(" at ")[0] if err else "returned")}


NS_T45 = list(range(0, 9)) + [12, 16, 24]


def entity_arrays(img, va, depth=1, seen=None):
    """Entity 0 of the arrays a method handler (or a resolver it calls) indexes with its entref:
    `imul r, r, S` followed by `add r, B` / `lea r, [r + B]` (array at B) or
    `add r, dword ptr [x*4 + P]` (array pointer at P). [(address, S)]"""
    seen = seen if seen is not None else set()
    if va in seen or not img.is_code(va):
        return []
    seen.add(va)
    ins = img.body(va)[:120]
    out = []
    for k, i in enumerate(ins):
        m = re.match(r"^(e\w\w), \1, (0x[0-9a-f]+)$", i.op_str) if i.mnemonic == "imul" else None
        if not m:
            continue
        r, st = m.group(1), int(m.group(2), 16)
        if not 0x40 <= st <= 0x2000:
            continue
        for j in ins[k + 1:k + 4]:
            m2 = re.match(r"^%s, (0x[0-9a-f]+)$" % r, j.op_str) if j.mnemonic == "add" else None
            m3 = re.match(r"^%s, \[%s \+ (0x[0-9a-f]+)\]$" % (r, r), j.op_str) \
                if j.mnemonic == "lea" else None
            m4 = re.match(r"^%s, dword ptr \[e\w\w\*4 \+ (0x[0-9a-f]+)\]$" % r, j.op_str) \
                if j.mnemonic == "add" else None
            b = None
            if m2 or m3:
                b = int((m2 or m3).group(1), 16)
            elif m4 and img.in_image(int(m4.group(1), 16)):
                b = img.u32(int(m4.group(1), 16))
            if b is not None and (img.in_image(b) or b < 0x100000 - st):
                out.append((b, st))
                break
            # base and field folded: `cmp dword ptr [r + B+f], 0`: fill around it
            m5 = re.search(r"\[%s \+ (0x[0-9a-f]+)\]" % r, j.op_str)
            if m5 and img.in_image(int(m5.group(1), 16)):
                b = (int(m5.group(1), 16) & ~3) - st + 4
                out.append((b, 2 * st))
                break
    if depth:
        for t in img.direct_calls(va, jumps=False)[:8]:
            out += entity_arrays(img, t, depth - 1, seen)
    return sorted(set(out))


def handler_runs(runner, e, inst, layout, method=False):
    """Types first: run with 16 parameters, give each index whose type an error named a value of
    that type, run again (at most 6 passes); then the arity runs with those values."""
    slots = {}
    arrays = entity_arrays(runner.img, e["handler"]) if method else []
    for _ in range(6):
        r = runner.run(e["handler"], inst, 16, slots, arrays)
        new = {int(i): v[0] for i, v in r["types"].items() if int(i) not in slots}
        if not new:
            break
        slots.update(new)
    if layout == "t6":
        ns = sorted({e["min"], min(e["max"], 16), min(e["max"], 16) + 1})
    else:
        ns = NS_T45
    runs = [runner.run(e["handler"], inst, n, slots, arrays) for n in ns]
    # the largest N again with the numbers at 0: its reads are added to that run's (marked)
    z = runner.run(e["handler"], inst, ns[-1], slots, arrays, zero=True)
    extra = sorted(set(z["reads"]) - set(runs[-1]["reads"]))
    if extra and not z["missing"]:
        runs[-1]["reads"] = sorted(set(runs[-1]["reads"]) | set(extra))
        runs[-1]["reads_with_zero_values"] = extra
    return {"slots": slots, "runs": runs, "self_arrays": arrays}


# ---------------------------------------------------------------------------------------------
def extract(game, exe, out_path, with_handlers=True, only=None):
    prof = PROFILES[game]
    log = []
    exe = exe or paths.need("exe." + game)  # tools/re/paths.local.json or GSC_RE_EXE_<GAME>
    img = Image(exe)
    log.append(img.describe())
    layout, st = prof["layout"], STRIDE[prof["layout"]]
    roles_def = prof["roles"] or find_emitters(img, prof, log)
    lk = Lookup(img)
    roles = {}
    for kind, (start, join, regs) in roles_def.items():
        anchor = check_role_anchor(img, prof, kind, start, join)
        for inst, vmname in INSTS.items():
            fns = run_role(img, start, join, regs, inst)
            tabs = []
            for f in fns:
                for a, ents in trace_tables(img, lk, f, layout):
                    tabs.append({"lookup": f, "va": a, "count": len(ents),
                                 "static_bound": static_bound(img, a, st),
                                 "entries": ents})
            if not tabs:
                die("%s_%s: lookups %s scan no table" % (vmname, kind, fns))
            roles["%s_%s" % (vmname, kind)] = {"path": [start, join], "inst": inst,
                                               "anchor": anchor, "lookups": fns, "tables": tabs}
            log.append("%s_%s: path %#x, lookups %s, tables %s" % (
                vmname, kind, start, [hex(f) for f in fns],
                [(hex(t["va"]), t["count"], t["static_bound"]) for t in tabs]))
    # round trip
    for rname, ro in roles.items():
        seen = {}
        for ti, t in enumerate(ro["tables"]):
            for k, e in enumerate(t["entries"]):
                e["table"], e["index"] = ti, k
                got = None
                for f in ro["lookups"]:
                    h, outs = lk.call(f, e["name"])
                    if h:
                        got = (f, h, outs)
                        break
                if got is None:
                    die("%s: %s not found again" % (rname, e["name"]))
                f, h, outs = got
                rt = {"lookup": f, "handler": h, "name_ptr": outs["name_ptr"],
                      "dev": None if outs["dev"] == 0xEEEEEEEE else outs["dev"]}
                if layout == "t6":
                    rt["min"], rt["max"] = outs["min"], outs["max"]
                e["round_trip"] = rt
                same = h == e["handler"] and outs["name_ptr"] == e["name_ptr"]
                if layout == "t6":
                    same = same and (outs["min"], outs["max"]) == (e["min"], e["max"])
                if e["name"] in seen:
                    e["shadowed"] = True
                    if not h == seen[e["name"]]:
                        die("%s: %s returns %#x, not the first entry's" % (rname, e["name"], h))
                else:
                    if not same:
                        die("%s: round trip of %s: %s vs entry %s" % (rname, e["name"], rt, e))
                    seen[e["name"]] = h
        log.append("%s: round trip ok on %d names (%d shadowed entries)" % (
            rname, len(seen), sum(1 for t in ro["tables"] for e in t["entries"]
                                  if e.get("shadowed"))))
    vm = None
    if with_handlers:
        vm = find_vm(img, log)
        runner = Runner(img, vm)
        crashed = []
        n = 0
        for rname, ro in roles.items():
            for t in ro["tables"]:
                for e in t["entries"]:
                    if only and e["name"] not in only:
                        continue
                    try:
                        hr = handler_runs(runner, e, ro["inst"], layout,
                                          rname.endswith("method"))
                    except OSError as ex:  # unicorn itself faulted: fresh emulator, no runs
                        crashed.append("%s %s: %s" % (rname, e["name"], ex))
                        runner = Runner(img, vm)
                        continue
                    e["slots"], e["runs"] = hr["slots"], hr["runs"]
                    e["self_arrays"] = hr["self_arrays"]
                    n += 1
        log.append("handlers run: %d entries; the emulator crashed on %d (recorded without "
                   "runs): %s" % (n, len(crashed), "; ".join(crashed) or "none"))
    out = {"game": game, "exe": os.path.basename(exe), "what": prof["what"],
           "pe_timestamp": img.timestamp, "base": img.base, "layout": layout, "roles": roles,
           "vm": {k: v for k, v in (vm or {}).items() if k != "acc_ranges"}, "log": log}
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "w") as f:
        json.dump(out, f, indent=1, default=lambda o: sorted(o) if isinstance(o, set) else str(o))
    print("\n".join(log))
    print("wrote", out_path)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    x = sub.add_parser("extract")
    x.add_argument("--game", required=True, choices=sorted(PROFILES))
    x.add_argument("--exe")
    x.add_argument("--out")
    x.add_argument("--no-handlers", action="store_true")
    x.add_argument("--only", help="comma-separated names (handler runs only for these)")
    a = ap.parse_args()
    extract(a.game, a.exe, a.out or os.path.join(OUT, "%s_extract.json" % a.game),
            not a.no_handlers, set(a.only.split(",")) if a.only else None)


if __name__ == "__main__":
    main()
