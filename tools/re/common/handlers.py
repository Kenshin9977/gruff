"""Parameter types of builtin handlers: the accessors found by their own error strings, the
script-error function, and a runner that executes one handler with N parameters under Unicorn.

Engine-neutral. What an engine supplies (a dict, `vm`):
    vmpub, vmpub_stride, vmpub_top, vmpub_outparamcount   scrVmPub[inst] (RVA, stride, offsets)
    handler_args(inst, method, self_ref) -> args tuple     the handler's calling convention
    index_arg                                              accessor argument holding the index
    out_arg (optional)                                     accessor argument of a vector out
"""
import collections
import struct

from .emu import Emu, unicorn

ACCESSOR_LABELS = [  # error string fragment -> parameter type (first match wins)
    ("is not an int, param", "int"),
    ("is not an uint64, param", "uint64"),
    ("is not a float, param", "float"),
    ("is not a vector, param", "vector"),
    ("is not an IString, param", "istring"),
    ("is not a localized string", "istring"),
    ("is not a function, param", "function"),
    ("is not an entity, param", "entity"),
    ("is not an object, param", "object"),
    ("is not an animtree, param", "animtree"),
    ("unable to find animation", "anim"),
    ("is not a valid weapon", "weapon"),
    ("illegal team string", "team"),
    ("is not a hash", "hash"),
    ("cannot cast %s to string", "string"),
    ("is not a string", "string"),
    ("is not an int", "int"),
    ("is not a float", "float"),
    ("is not a vector", "vector"),
    ("is not an entity", "entity"),
    ("is not a function", "function"),
]
PARAM_MISSING = "parameter %d does not exist"
POINTER_LABELS = ("string", "istring", "entity", "object", "weapon", "function", "anim",
                  "animtree", "team")


def handler_callees(img, handlers):
    """Counter of the direct call/jmp targets of every handler (outside the handler)."""
    calls = collections.Counter()
    for h in handlers:
        for t in img.direct_calls(h):
            calls[t] += 1
    return calls


def reads_param_count(img, va, vm):
    """Does the function at va (or a direct callee) read scrVmPub[inst].outparamcount (the
    parameter bound check)? The address is the image's (rip-relative, or base register + rva)."""
    rva = vm["vmpub"] + vm["vmpub_outparamcount"]
    pat = "0x%x]" % rva
    for f in [va] + img.direct_calls(va):
        for ins in img.body(f):
            if img.operand_addr(ins) == img.base + rva or pat in ins.op_str:
                return True
    return False


ARG_REGS = {"ecx": 0, "rcx": 0, "edx": 1, "rdx": 1, "r8d": 2, "r8": 2, "r9d": 3, "r9": 3}


def guess_index_arg(img, va, default=1):
    """The argument that carries the parameter index into the bound-checked getter: the first
    `mov r8d|edx, <arg register>` before the first call (Scr_GetEntity(index) with an implicit
    inst moves ecx into r8d)."""
    for ins in img.body(va)[:16]:
        if ins.mnemonic == "call":
            break
        if ins.mnemonic == "mov":
            dst, _, src = ins.op_str.partition(", ")
            if dst in ("r8d", "edx") and src in ARG_REGS:
                return ARG_REGS[src]
    return default


def find_accessors(img, handlers, log, extra_markers=(), resolver_min_callers=20, vm=None):
    """Parameter accessors and entref resolvers among the functions the handlers call.

    accessor: loads "parameter %d does not exist" itself, or loads a "type %s is not a X, param
    %d" error (the bound check sits in a sibling), or is a short wrapper whose first call or jmp
    is an accessor (Scr_GetString = GetConstString + convert). Its type is the error it raises.
    resolver: loads "not an entity" and is called by many handlers: a method's entref -> entity.
    With `vm` given, a "not an entity" function that reads outparamcount (itself or through a
    callee) is a parameter accessor of type entity, not a resolver (T7's Scr_GetEntity(index));
    its index argument is recorded as "index_arg".
    """
    calls = handler_callees(img, handlers)
    markers = (", param %d", "is not a valid weapon", "illegal team string") + tuple(extra_markers)
    acc, resolvers = {}, {}
    cands = [t for t, n in calls.most_common()]
    for t in cands:
        own = img.strings_loaded(t, depth=0)
        if any(PARAM_MISSING in s for s in own) or any(m in s for s in own for m in markers):
            strs = img.strings_loaded(t, depth=1)
            label = "any"
            for frag, lab in ACCESSOR_LABELS:
                if any(frag in s for s in strs):
                    label = lab
                    break
            acc[t] = {"label": label, "callers": calls[t]}
        elif calls[t] >= resolver_min_callers and any(s == "not an entity" for s in own):
            if vm is not None and reads_param_count(img, t, vm):
                acc[t] = {"label": "entity", "callers": calls[t],
                          "index_arg": guess_index_arg(img, t, vm.get("index_arg", 1))}
            else:
                resolvers[t] = {"label": "self", "callers": calls[t]}
    for t in cands:  # wrappers
        if t in acc or t in resolvers:
            continue
        if img.end_of(t) - t > 0x40:
            continue
        body = img.body(t)
        first = [i for i in body[:4] if i.mnemonic in ("call", "jmp") and i.op_str.startswith("0x")]
        if first and int(first[0].op_str, 16) in acc:
            inner = int(first[0].op_str, 16)
            acc[t] = dict(acc[inner], wrapper_of=inner, callers=calls[t])
    log.append("accessors: %d (%s); entref resolvers: %d" % (
        len(acc), dict(collections.Counter(a["label"] for a in acc.values())), len(resolvers)))
    return acc, resolvers


def scr_error_candidates(img, acc):
    """Scr_Error: in the accessors' error paths, the call that follows the message formatter
    (`call va; ...; call Scr_Error`). The pair seen most often names it."""
    pairs = collections.Counter()
    for a in acc:
        prev = None
        for ins in img.body(a):
            if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                if prev is not None:
                    pairs[(prev, t)] += 1
                prev = t
    if not pairs:
        raise SystemExit("Scr_Error: no call pair in the accessors")
    (fmt, err), n = pairs.most_common(1)[0]
    return [err]


class HandlerRunner:
    """Runs one handler with N parameters. scrVmPub[inst] gets outparamcount N and a top pointer
    into a fake value stack; accessors are hooked at entry (record index, return a neutral value);
    an unmapped read or write maps a zero page so the handler keeps going; Scr_Error stops it.

    `self_obj(inst, method)`, when given, returns the entref / object the handler gets (a fake
    entity built by the engine profile); `on_resolver` the value the entref resolver returns."""

    def __init__(self, img, vm, accessors, resolvers, stops, self_obj=None, pointer_value=None):
        self.img, self.vm, self.acc, self.res = img, vm, accessors, resolvers
        self.emu = Emu(img, null_page=True)
        uc = self.emu.uc
        self.events = []
        self.vstack = self.emu.alloc(0x10000)
        self.blank = pointer_value or self.emu.alloc(0x10000)  # neutral pointer target
        self.self_obj = self_obj
        for a in accessors:
            uc.hook_add(unicorn.UC_HOOK_CODE, self._on_acc, begin=a, end=a)
        for a in resolvers:
            uc.hook_add(unicorn.UC_HOOK_CODE, self._on_res, begin=a, end=a)
        for s in stops:
            uc.hook_add(unicorn.UC_HOOK_CODE, self._on_stop, begin=s, end=s)
        self.emu.map_on_unmapped()
        self.stop_reason = None
        self.resolver_value = None
        self.entity_value = None  # what an entity parameter accessor returns (a fake entity)

    def _on_acc(self, uc, addr, size, ud):
        a = self.acc[addr]
        idx = self.emu.arg32(a.get("index_arg", self.vm.get("index_arg", 1)))
        self.events.append((a["label"], idx))
        lab = a["label"]
        if lab == "vector" and "out_arg" in self.vm:  # Scr_GetVector(inst, index, float out[3])
            try:
                uc.mem_write(self.emu.arg(self.vm["out_arg"]), b"\0" * 12)
            except unicorn.UcError:
                pass
        val = self.blank if lab in POINTER_LABELS else 0
        if lab == "entity" and self.entity_value:
            val = self.entity_value
        self.emu.ret(val, self.vm.get("accessor_pop", 0))

    def _on_res(self, uc, addr, size, ud):
        self.events.append(("self", -1))
        self.emu.ret(self.resolver_value or self.blank)

    def _on_stop(self, uc, addr, size, ud):
        self.stop_reason = "script error at %#x" % addr
        uc.emu_stop()

    def pub(self, inst):
        """scrVmPub[inst]."""
        return self.img.base + self.vm["vmpub"] + inst * self.vm.get("vmpub_stride", 0)

    def start_run(self, inst, nparams, fill=None):
        """A fresh VM stack (`fill(top)` may write its slots) with `nparams` parameters, and the
        run's records cleared."""
        uc = self.emu.uc
        vm = self.vm
        pub = self.pub(inst)
        top = self.vstack + 0x8000
        uc.mem_write(self.vstack, b"\0" * 0x10000)
        if fill:
            fill(top)
        self.emu.wptr(pub + vm["vmpub_top"], top)
        uc.mem_write(pub + vm["vmpub_outparamcount"], struct.pack("<I", nparams))
        self.events = []
        self.stop_reason = None
        self.emu.pages = 0

    def run(self, handler, inst, method, nparams, count=300_000):
        vm = self.vm
        self.start_run(inst, nparams)
        self_ref = 0
        if self.self_obj:
            self_ref, self.resolver_value = self.self_obj(self, inst, method)
        args = vm["handler_args"](inst, method, self_ref)
        r, err = self.emu.run(handler, args, count=count)
        return list(self.events), self.stop_reason or err or "returned"


def handler_profile(runner, handler, inst, method, mn, mx, cap_n=16):
    """Indices and types read with N = min and N = cap(max). Returns a dict of facts."""
    if mn >= 0x80000000 or mn < 0:  # -1/-1: no arity recorded in the entry
        mn, mx = 0, cap_n
    cap = mx if mx <= cap_n else max(mn, cap_n)
    ev_max, why_max = runner.run(handler, inst, method, cap)
    ev_min, why_min = runner.run(handler, inst, method, mn) if mn != cap else (ev_max, why_max)
    types = collections.defaultdict(set)
    for lab, i in ev_max:
        if 0 <= i < 64:
            types[i].add(lab)
    read_max = sorted(types)
    read_min = sorted({i for lab, i in ev_min if 0 <= i < 64})
    return {
        "n_max": cap, "read_at_max": read_max, "stop_at_max": why_max,
        "n_min": mn, "read_at_min": read_min, "stop_at_min": why_min,
        "types": {i: sorted(s) for i, s in types.items()},
        "resolves_self": any(lab == "self" for lab, i in ev_max),
    }


def param_types(hp):
    """Positional types read by the handler with N = cap(max) parameters, '?' when not read."""
    if not hp:
        return None
    t = {int(k): v for k, v in hp["types"].items()}
    if not t:
        return []
    n = max(t) + 1
    return ["|".join(t[i]) if i in t else "?" for i in range(n)]
