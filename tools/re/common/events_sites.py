"""Call sites of the notifiers: the Scr_Add* pushers (static push cores, then each pushing call
run under Unicorn) and the guided-path measure of each site (see common/events.py)."""
import re

from .events import TOKEN_BASE, TOKEN16, PROBE_RET

REG_OF_ARG64 = {0: ("rcx", "ecx"), 1: ("rdx", "edx"), 2: ("r8", "r8d"), 3: ("r9", "r9d")}


class PushCores:
    """The functions that push one value on the VM stack, recognised statically: an
    `add qword ptr [... + top_disp], value_size` of the stack top followed, in the same
    function, by `mov dword ptr [reg + type_off], imm` (the type tag). A set-like object tested
    lazily: `va in cores`; `cores.type_of(va)` is the tag."""

    def __init__(self, img, top_disp, value_size, type_off, width="qword", top_abs=None,
                 tag_width="dword", top_store=False):
        """top_abs: the stack top's address when the VM addresses it directly (rip-relative or
        absolute: IW engines, one VM). top_disp None: the top is reached through a pointer
        (`add qword ptr [reg], size`: T11's [[ctx + 0x688]]). tag_width: the tag's store (T11:
        a byte)."""
        self.img = img
        self.top_abs = top_abs
        self.top_store = top_store
        self.size_pat = re.compile(r", (?:%s|%d)$" % (hex(value_size), value_size))
        def num(v):  # capstone writes 0..9 in decimal, larger values in hex
            return "(?:%s|%d)" % (hex(v), v)
        if top_disp is None:
            self.add_pat = re.compile(r"^%s ptr \[\w+\], %s$" % (width, num(value_size)))
        else:
            self.add_pat = re.compile(r"^%s ptr \[.*\+ %s\], %s$" % (width, num(top_disp),
                                                                     num(value_size)))
        off = (r" \+ %s" % num(type_off)) if type_off else ""
        self.tag_pat = re.compile(r"^%s ptr \[[re]\w+%s\], (0x[0-9a-f]+|\d+)$" % (tag_width, off))
        # BO6 advances the top by a store: `lea rax, [rcx + 0x10]; mov [ctx + top_disp], rax`
        self.store_pat = None if top_disp is None else re.compile(
            r"^qword ptr \[\w+ \+ %s\], r\w+$" % num(top_disp))
        self.cache = {}

    def type_of(self, va):
        if va not in self.cache:
            self.cache[va] = self._scan(va)
        return self.cache[va]

    def __contains__(self, va):
        """A push core, or a short function that calls or jumps to one (Scr_AddEntityNum ->
        Scr_AddObject): a pusher run lets those run."""
        if va is None:
            return False
        return self.via(va, 2)

    def via(self, va, depth):
        """A core, a stack-top increment, or a short function reaching one within `depth`
        direct calls (T5: Scr_AddEntity -> Scr_AddEntityNum -> Scr_AddObject)."""
        if self.type_of(va) is not None or self.has_add(va):
            return True
        if depth <= 0:
            return False
        key = ("via", va, depth)
        if key not in self.cache:
            self.cache[key] = False  # recursion guard
            ok = False
            if self.img.end_of(va, 0x400) - va <= 0x400:
                ok = any(self.via(t, depth - 1) for t in self.img.direct_calls(va))
            self.cache[key] = ok
        return self.cache[key]

    def _insns(self, va):
        img = self.img
        f = img.func(va)
        if f and f[0] != va:
            return []
        end = f[1] if f else img.end_of(va, 0x400)
        if end - va > 0x400:
            return []
        return list(img.md.disasm(img.d[va - img.base:end - img.base], va))

    def _is_add(self, ins):
        if self.top_store and ins.mnemonic == "mov" and self.store_pat.match(ins.op_str):
            return True
        return ins.mnemonic == "add" and (self.add_pat.match(ins.op_str) or (
            self.top_abs is not None and self.img.operand_addr(ins) == self.top_abs and
            self.size_pat.search(ins.op_str)))

    def has_add(self, va):
        """The function advances the stack top itself (IW4's Scr_IncInParam: the callers write
        the tag)."""
        key = ("add", va)
        if key not in self.cache:
            self.cache[key] = any(self._is_add(i) for i in self._insns(va))
        return self.cache[key]

    def _scan(self, va):
        seen_add = False
        for ins in self._insns(va):
            if self._is_add(ins) or (ins.mnemonic == "call" and ins.op_str.startswith("0x") and
                                     self.has_add(int(ins.op_str, 16))):
                seen_add = True
            elif seen_add and ins.mnemonic == "mov":
                m = self.tag_pat.match(ins.op_str)
                if m:
                    return int(m.group(1), 0)
        return None


class Pushers:
    """Scr_Add* recognised by running them: the VM stack top (per inst) points into a buffer and
    the call runs with every inner call skipped except those to the push cores; a call that
    advances the top by exactly one value has pushed the type tag written at the new top (the
    core's static tag is the second measure). The pushed value, when it is the return of a
    skipped call, names its source: a pointer whose source is one of `entity_fns` (the
    entity-id resolvers) is an entity."""

    def __init__(self, img, probe, cores, top_va, value_size, type_off, typenames,
                 entity_fns=(), insts=(0, 1), tag_width="dword"):
        self.img, self.probe, self.cores = img, probe, cores
        self.top_va, self.value_size, self.type_off = top_va, value_size, type_off
        self.typenames = typenames
        self.entity_fns = set(entity_fns)
        self.insts = insts
        self.tag_mask = 0xFF if tag_width == "byte" else 0xFFFFFFFF
        self.cache = {}
        self.buf = probe.emu.alloc(0x2000)
        self.static_disagree = set()

    def classify(self, target, args):
        """(vm type name, entity flag, source va or None) or None."""
        key = (target, args[0] if args else None)
        if key not in self.cache:
            self.cache[key] = self._run(target, args)
        return self.cache[key]

    def _run(self, target, args):
        """Skipped calls return a probe value first (so the pushed value names its source);
        when nothing is pushed that way (an inst number returned by a skipped call, then used
        as an index), again with skipped calls returning 0."""
        if target is None or not self.img.is_code(target):
            return None
        out = self._run1(target, args)
        if out is None:
            self.probe.zero_returns = True
            try:
                out = self._run1(target, args)
            finally:
                self.probe.zero_returns = False
        return out

    def _run1(self, target, args):
        pr = self.probe
        emu = pr.emu
        uc = pr.uc
        saved = {}
        base = self.buf + 0x800
        for inst in self.insts:
            va = self.top_va(inst)
            saved[va] = bytes(uc.mem_read(va, emu.img.ptr))
            emu.wptr(va, base)
        uc.mem_write(self.buf, bytes(0x2000))
        pr.calls = {}
        pr.window = None
        pr.capture = None
        pr.target = None
        pr.guide = None
        pr.skipped = []
        pr.keep_calls = self.cores
        pr.reset_regs()
        emu.pages = 0
        try:
            regs = None
            if not emu.x64 and len(args) >= 7:  # x86 register arguments (usercall: inst in eax)
                from .emu import X as _X
                regs = {_X.UC_X86_REG_ECX: args[4], _X.UC_X86_REG_EDX: args[5],
                        _X.UC_X86_REG_EAX: args[6]}
            emu.run(target, list(args[:4]), count=20_000, regs=regs)
        finally:
            pr.keep_calls = None
        skipped = list(pr.skipped)
        out = None
        for inst in self.insts:
            va = self.top_va(inst)
            top = emu.rptr(va)
            if top == base + self.value_size:
                t = emu.r32(top + self.type_off) & self.tag_mask
                val = emu.r32(top + (4 if self.type_off == 0 else 0))
                name = self.typenames[t] if t < len(self.typenames) else "type%d" % t
                src = None
                k = (val - PROBE_RET) // 0x100
                if PROBE_RET <= val and not (val - PROBE_RET) % 0x100 and k < len(skipped):
                    src = skipped[k][1]
                ent = src in self.entity_fns or (src is None and any(
                    x[1] in self.entity_fns for x in skipped))
                out = (name, ent, src)
                break
        for va, b in saved.items():
            uc.mem_write(va, b)
        return out


SCHEMA_TYPE = {"int": "int", "integer": "int", "float": "float", "string": "string",
               "localized string": "istring", "istring": "istring", "vector": "vector",
               "undefined": "undefined"}


def schema_type(vm_type, entity):
    if vm_type in ("pointer", "object", "entity") and entity:
        return "entity"
    return SCHEMA_TYPE.get(vm_type, "unknown")


class SiteResolver:
    """Measures one call site of a notifier: a guided emulation from the function start (or
    the root of the paths reaching the site) to the notifier's entry, every inner call skipped
    and recorded; straight-line windows ending at the site as a fallback."""

    def __init__(self, img, dec, probe, notifiers, pushers, windows=(320, 160, 80, 40, 20, 10, 5, 2)):
        self.img, self.dec, self.probe = img, dec, probe
        self.notifier_vas = {n.va for n in notifiers}
        self.pushers = pushers
        self.windows = windows

    def cfg(self, insns):
        """Successors of each instruction of a linearly decoded function (direct edges only;
        an indirect jmp, ret, int3 or a jmp out of the function ends a path)."""
        addrs = {i.address for i in insns}
        succ = {}
        for k, ins in enumerate(insns):
            m = ins.mnemonic
            nxt = insns[k + 1].address if k + 1 < len(insns) else None
            tgt = int(ins.op_str, 16) if (m.startswith("j") and ins.op_str.startswith("0x")) else None
            if m == "jmp":
                s = [tgt] if tgt in addrs else []
            elif m.startswith("j"):
                s = [x for x in (nxt, tgt) if x in addrs]
            elif m in ("int3", "ud2") or m.startswith("ret"):
                s = []
            else:
                s = [nxt] if nxt is not None else []
            succ[ins.address] = s
        return succ

    def guided_plan(self, insns, site):
        """(start candidates, guide): the instructions that can reach the site (backward over
        the direct edges); the starts are the function start when it reaches the site, else the
        roots of that set (no predecessor in it: switch cases, a loop head), lowest address
        first; the guide forces each conditional branch with only one side reaching the site."""
        succ = self.cfg(insns)
        pred = {}
        for a, ss in succ.items():
            for b in ss:
                pred.setdefault(b, []).append(a)
        R = {site}
        stack = [site]
        while stack:
            b = stack.pop()
            for a in pred.get(b, ()):
                if a not in R:
                    R.add(a)
                    stack.append(a)
        guide = {}
        for a in R:
            ss = succ.get(a, [])
            if len(ss) == 2:
                ok = [b for b in ss if b in R]
                if len(ok) == 1:
                    guide[a] = ok[0]
        starts = []
        if insns[0].address in R:
            starts.append(insns[0].address)
        roots = sorted(a for a in R if not any(p in R for p in pred.get(a, ())) and
                       a != insns[0].address)
        starts += roots
        return starts, guide, R

    def resolve(self, site, n):
        fstart, insns, idx = self.dec.at(site)
        res = {"site": site, "notifier": n.va, "fn": fstart}
        if insns is None:
            res["fail"] = "site not decoded from its function start"
            return res
        pr = self.probe
        pr.calls = {}
        pr.index_calls(fstart, insns)
        starts, guide, R = self.guided_plan(insns, site)
        fend = insns[-1].address
        got = None
        tries = 0
        for st in starts[:6]:
            tries += 1
            r = pr.run_window(st, site, n, count=60_000, window=(fstart, fend), guide=guide)
            if r is not None:
                got = r
                res["start"] = "function start" if st == fstart else "path root"
                break
        if got is None:  # straight-line windows ending at the site
            for k in self.windows:
                si = max(0, idx - k)
                r = pr.run_window(insns[si].address, site, n, guide=guide)
                if r is not None:
                    got = r
                    res["start"] = "window %d" % (idx - si)
                    break
        res["alternatives"] = self.static_imms(insns, idx, n.event)
        res["static_count"] = self.static_imms(insns, idx, n.count, zero_xor=True)
        if got is None:
            res["fail"] = "notifier entry not reached from the site (%d path starts, %d windows)" % (
                tries, len(self.windows))
            return res
        vals, skipped = got
        res.update(vals)
        # pushes after the last notify call on the path belong to this one
        last = 0
        for k, (addr, tgt, args) in enumerate(skipped):
            if tgt in self.notifier_vas:
                last = k + 1
        skipped = skipped[last:]
        pushes = []
        for k, (addr, tgt, args) in enumerate(skipped):
            c = self.pushers.classify(tgt, args) if self.pushers else None
            if c is not None and c[2] is None and k and \
                    skipped[k - 1][1] in self.pushers.entity_fns:
                # the id comes in a register from the call just before (IW3 Scr_AddObject(eax)
                # after Scr_GetEntityId): that call is the source
                c = (c[0], True, skipped[k - 1][1])
            if c is not None:
                pushes.append({"at": addr, "fn": tgt, "vm_type": c[0], "entity": c[1],
                               "source": c[2], "type": schema_type(c[0], c[1])})
        res["pushes"] = pushes
        res["skipped"] = len(skipped)
        res["calls"] = [(tgt - self.img.base) if tgt else None for addr, tgt, args in skipped]
        return res

    def static_imms(self, insns, idx, spec, zero_xor=False):
        """Immediates written to an argument (spec ("arg", i)) between the last call before the
        site and the site: several when a branch picks the value. x64 only."""
        if not spec or spec[0] != "arg" or not self.img.x64:
            return []
        i = spec[1]
        regs = REG_OF_ARG64.get(i, ("[rsp + 0x%x]" % (0x20 + 8 * (i - 4)),))
        out = []
        for j in range(idx - 1, max(-1, idx - 40), -1):
            ins = insns[j]
            if ins.mnemonic == "call":
                break
            dst, _, src = ins.op_str.partition(", ")
            hit = any(dst == r or dst.endswith(r) for r in regs)
            if ins.mnemonic == "mov" and hit and (src.startswith("0x") or src.isdigit()):
                out.append(int(src, 0))
            elif zero_xor and ins.mnemonic == "xor" and hit and src == dst:
                out.append(0)
        return sorted(set(out))


def token_name(v, slots_by_token, width=4):
    base = TOKEN_BASE if width == 4 else TOKEN16
    mask = 0xFFFFFFFF if width == 4 else 0xFFFF
    return slots_by_token.get((v & mask) - base)
