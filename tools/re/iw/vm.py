"""The script VM's parameter interface, found from the handlers alone (no names, no error
strings: most IW builds strip both), then each handler run under emulation for every parameter
count N = 0..NMAX to measure its arity and its parameter types.

1. Callees: the functions the handlers call directly, by number of handlers calling them.
2. VM globals: the parameter count (scrVmPub.outparamcount, `G`) and the stack top
   (scrVmPub.top, `T`): among the globals the frequent callees reference, G is the dword most
   often compared (`cmp`), T the pointer most often loaded. Checked by the probes below: an
   accessor reads the fake stack at T - k*stride only when G > k.
3. Accessors: each frequent callee is run on a fake VM stack (T -> a buffer of slots, G = 12),
   with every candidate argument location holding a distinct small value: the slot it reads
   tells it is an accessor and where it takes its index (x64: rcx; x86 builds use custom
   conventions after whole-program optimisation). Then it is run once per value type t, every
   dword of every slot = t: the types it returns on are the types it accepts. The type names
   come from the VM's own type-name table when the build has one.
4. Error functions: an accessor run with G = 0 takes its error path; the functions it enters
   there and not on success, which never return when called, are the error raisers
   (Scr_Error, Scr_ParamError...). Hooked in every handler run: entering one stops the run.
5. Handler runs: accessors hooked at entry (index and type recorded, a neutral value
   returned), error raisers hooked, reads of G counted (the handler's own numparam checks).
   ok(N): the run returned, no error raiser entered, every index read < N. min = the least ok
   N, max = the greatest index read in an ok run + 1; variadic when an ok run at NMAX reads
   index NMAX-1 (a loop over all parameters); `max_checked` when a larger N errors.
"""
import bisect
import collections
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import user_site  # noqa: E402
from iwemu import Emu, STACK_SZ  # noqa: E402
from xdis import capstone_for  # noqa: E402
from common.image import rel32_branches  # noqa: E402

user_site()
import numpy as np  # noqa: E402
import unicorn  # noqa: E402
from unicorn import x86_const as X  # noqa: E402

NMAX = 16
SLOTS = 40


# --------------------------------------------------------------------------- function starts
class Functions:
    """Function starts: .pdata (x64) plus direct call targets (both)."""

    def __init__(self, im):
        self.im = im
        s = set()
        for b, _ in im.pdata:
            s.add(im.base + b)
        for sva, vsz, op, t in rel32_branches(im.mem, im.base, im.sections, tail=5):
            hit = (op == 0xE8) & (t >= im.base + sva) & (t < im.base + sva + vsz)
            s.update(int(x) for x in np.unique(t[hit]))
        self.starts = sorted(s)
        self.set = set(s)

    def containing(self, va):
        k = bisect.bisect_right(self.starts, va) - 1
        return self.starts[k] if k >= 0 else None


def md_for(im):
    return capstone_for(im, detail=False)


def body(im, md, va, limit=0x600):
    """Linear decode from va to the first ret/int3 run (or limit)."""
    o = va - im.base
    out = []
    for ins in md.disasm(im.mem[o:o + limit], va):
        out.append(ins)
        if ins.mnemonic in ("ret", "int3") or (ins.mnemonic == "jmp" and not out[:-1]):
            break
    return out


def direct_callees(im, md, va):
    out = []
    for ins in body(im, md, va):
        if ins.mnemonic in ("call", "jmp") and ins.op_str.startswith("0x"):
            t = int(ins.op_str, 16)
            if im.is_code(t) and not (va <= t < va + 0x600 and ins.mnemonic == "jmp"):
                out.append(t)
    return out


def global_refs(im, md, va):
    """[(mnemonic, global va, size word)] for every absolute/rip-relative memory operand."""
    out = []
    for ins in body(im, md, va, 0x300):
        s = ins.op_str
        if "[" not in s:
            continue
        inner = s[s.index("[") + 1:s.index("]")]
        size = s[:s.index("[")].strip().split(" ")[-2] if " ptr " in s[:s.index("[") + 1] or "ptr [" in s else ""
        g = None
        if "rip" in inner:
            try:
                disp = inner.replace("rip", "").replace(" ", "")
                g = ins.address + ins.size + int(disp, 16) if disp else ins.address + ins.size
            except ValueError:
                g = None
        elif inner.startswith("0x"):
            try:
                g = int(inner, 16)
            except ValueError:
                g = None
        if g is not None and im.in_image(g) and not im.is_code(g):
            out.append((ins.mnemonic, g, size, ins.op_str))
    return out


# --------------------------------------------------------------------------- the VM
class VM:
    def __init__(self, im, handlers, log=print, fns=None, ctx_mode=False):
        """handlers: [(va, kind)] kind 'function'/'method'. ctx_mode: the VM state is a
        context structure passed in rcx to every handler and accessor (IW8 and later), not
        globals: G and T are fields of a heap context."""
        self.im = im
        self.log = log
        self.md = md_for(im)
        self.fns = fns or Functions(im)
        self.emu = Emu(im)
        self.emu.install_import_stubs()
        if not im.x64:
            self.emu.stub_crt_ptd(log)
        self.handlers = handlers
        self.stride = 16 if im.x64 else 8
        self.type_names = {}
        self.errors = []
        self.strict = True
        self.alt_neutral = False
        self.ctx_mode = ctx_mode
        self.ctx = self.emu.alloc(0x80000, 0x1000) if ctx_mode else None

    # ---- step 1-2
    def find_globals(self):
        im, md = self.im, self.md
        freq = collections.Counter()
        for h, _ in self.handlers:
            for c in set(direct_callees(im, md, h)):
                freq[c] += 1
        self.callee_freq = freq
        top = [c for c, n in freq.most_common(200) if n >= 3]
        cmp_d = collections.Counter()
        load_p = collections.Counter()
        for c in top:
            seen = set()
            for mn, g, size, ops in global_refs(im, md, c):
                if (mn, g) in seen:
                    continue
                seen.add((mn, g))
                if mn == "cmp" and "dword" in ops:
                    cmp_d[g] += 1
                if mn == "mov" and (("qword" in ops) if im.x64 else ("dword" in ops)):
                    load_p[g] += 1
        if self.ctx_mode:
            cmp_d, load_p = collections.Counter(), collections.Counter()
            for c in top:
                seen = set()
                for ins in body(im, md, c, 0x200):
                    st = ins.op_str
                    # [reg + disp]: the context may sit in rcx or in the register it was copied to
                    m = re.search(r"\[(r[a-z0-9]+) \+ 0x([0-9a-f]+)\]", st)
                    if not m or m.group(1) in ("rsp", "rbp", "rip"):
                        continue
                    off = int(m.group(2), 16)
                    if (ins.mnemonic, off) in seen or off >= 0x80000:
                        continue
                    seen.add((ins.mnemonic, off))
                    if ins.mnemonic == "cmp" and "dword" in st:
                        cmp_d[self.ctx + off] += 1
                    if ins.mnemonic == "mov" and "qword" in st:
                        load_p[self.ctx + off] += 1
        self.cand_G = [g for g, _ in cmp_d.most_common(6)]
        self.cand_T = [g for g, _ in load_p.most_common(10)]
        self.log("vm: %d distinct callees of %d handlers; G candidates %s; T candidates %s" % (
            len(freq), len(self.handlers), [hex(g) for g in self.cand_G[:4]],
            [hex(g) for g in self.cand_T[:6]]))
        return top

    # ---- fake stack
    def _stack(self, fill_type=None, nslots=SLOTS):
        emu = self.emu
        if not hasattr(self, "fs_base"):
            self.fs_base = emu.alloc(self.stride * (2 * SLOTS + 8), 64)
            self.zero_buf = emu.alloc(0x10000, 64)
            emu.heap_mark = emu.heap_next
        base = self.fs_base
        top = base + self.stride * SLOTS
        if fill_type is None:
            emu.uc.mem_write(base, b"\0" * (self.stride * (2 * SLOTS + 8)))
        else:
            emu.uc.mem_write(base, struct.pack("<I", fill_type) * (self.stride * (2 * SLOTS + 8) // 4))
        emu.uc.mem_write(self.zero_buf, b"\0" * 0x10000)
        if self.ctx_mode:
            emu.uc.mem_write(self.ctx, bytes(0x80000))
            for pv, data in emu.patches.items():
                if self.ctx <= pv < self.ctx + 0x80000:
                    emu.uc.mem_write(pv, data)
        return base, top

    def _arg_locs(self):
        if self.im.x64:
            return ["rdx", "r8", "r9"] if self.ctx_mode else ["rcx", "rdx", "r8", "r9"]
        return ["eax", "ecx", "edx", "ebx", "esi", "edi", "s1", "s2"]

    _REG = {"rcx": X.UC_X86_REG_RCX, "rdx": X.UC_X86_REG_RDX, "r8": X.UC_X86_REG_R8,
            "r9": X.UC_X86_REG_R9, "eax": X.UC_X86_REG_EAX, "ecx": X.UC_X86_REG_ECX,
            "edx": X.UC_X86_REG_EDX, "ebx": X.UC_X86_REG_EBX, "esi": X.UC_X86_REG_ESI,
            "edi": X.UC_X86_REG_EDI}

    def _run_probe(self, fn, G, T, G_val, locvals, fill_type, track=False, count=20000):
        """Run fn with G=G_val, T=fake top, args from locvals {loc: value}. Returns
        (returned, slots read, slots whose value field was read, direct callees in order)."""
        emu, uc = self.emu, self.emu.uc
        emu.restore()
        base, top = self._stack(fill_type)
        emu.w32(G, G_val)
        emu.wptr(T, top)
        reads = set()
        vreads = set()
        lo, hi = base, base + self.stride * (2 * SLOTS + 8)
        st = self.stride
        vsize = 8 if self.im.x64 else 4

        def on_read(uc_, access, addr, size, value, data):
            if lo <= addr < hi:
                d = top - addr
                if d >= 0:
                    k = -(-d // st)
                    reads.add(k)
                    if addr - (top - k * st) < vsize:
                        vreads.add(k)
                else:
                    reads.add(-1)
        hooks = [uc.hook_add(unicorn.UC_HOOK_MEM_READ, on_read, begin=lo, end=hi)]
        direct = []
        path = []
        if track:
            fset = self.fns.set
            sp_reg = emu.sp_reg

            def on_block(uc_, addr, size, data):
                if len(path) < 4000:
                    path.append(addr)
                if addr in fset:
                    ret = emu.rptr(uc_.reg_read(sp_reg))
                    if fn <= ret < fn + 0x800:
                        direct.append(addr)
            hooks.append(uc.hook_add(unicorn.UC_HOOK_BLOCK, on_block))
        uc.ctl_flush_tb()  # a hook added after a block was translated never fires on it
        regs = {}
        stack_args = []
        for loc, v in locvals.items():
            if loc in self._REG:
                regs[self._REG[loc]] = v
            elif loc == "s1":
                stack_args.insert(0, v)
            elif loc == "s2":
                stack_args.append(v)
        if self.im.x64:
            args = [locvals.get(r, 0) for r in ("rcx", "rdx", "r8", "r9")]
            if self.ctx_mode:
                args[0] = self.ctx
            regs = {}
        else:
            args = stack_args
        try:
            emu.setup_call(fn, args, regs)
            emu.run(fn, count=count)
            ok = emu.returned
        except unicorn.UcError as e:
            ok = False
            self.last_error = (str(e), emu.uc.reg_read(emu.ip_reg))
        finally:
            for h in hooks:
                uc.hook_del(h)
            uc.ctl_flush_tb()
        self.last_vreads = vreads
        self.last_direct = direct
        self.last_path = tuple(path)
        return ok, reads, direct

    def static_users(self, va):
        """Function starts of the code referencing va (rip-relative or absolute)."""
        im = self.im
        out = set()
        if im.x64:
            from xdis import riprel_index
            for r in riprel_index(im, [(va, va + 8)]).get(va, []):
                f = self.fns.containing(r)
                if f:
                    out.add(f)
        else:
            pat = struct.pack("<I", va)
            i = im.mem.find(pat)
            while i >= 0:
                if im.is_code(im.base + i):
                    f = self.fns.containing(im.base + i)
                    if f:
                        out.add(f)
                i = im.mem.find(pat, i + 1)
        return out

    def probe_accessors(self, callees):
        """Classify callees: accessors (index location, accepted types), using G/T candidates."""
        locs = self._arg_locs()
        best = None
        for G in self.cand_G[:4]:
            for T in self.cand_T[:8]:
                if T == G:
                    continue
                n = 0
                for c in callees[:40]:
                    lv = {loc: i + 1 for i, loc in enumerate(locs)}
                    ok, reads, _ = self._run_probe(c, G, T, 12, lv, 6, count=5000)
                    if any(0 <= r <= len(locs) for r in reads):
                        n += 1
                if best is None or n > best[0]:
                    best = (n, G, T)
        n, G, T = best
        self.G, self.T = G, T
        self.log("vm: G (outparamcount) 0x%x, T (top) 0x%x: %d of the 40 most called callees read "
                 "the fake stack" % (G, T, n))
        acc = {}
        if not self.ctx_mode:
            extra = sorted(self.static_users(self.T) - set(callees))
            self.log("vm: %d more accessor candidates: functions referencing T" % len(extra))
            callees = list(callees) + extra
        else:
            # a context build has no global to cross-reference: every function a handler
            # calls directly is a candidate (the rarer accessors: dvar and asset hashes)
            extra = [c for c, n in self.callee_freq.most_common(3000) if c not in set(callees)]
            self.log("vm: %d more accessor candidates: every direct callee of a handler" % len(extra))
            callees = list(callees) + extra
        for c in callees:
            lv = {loc: i + 1 for i, loc in enumerate(locs)}
            ok, reads, _ = self._run_probe(c, G, T, 12, lv, 6)
            rs = sorted(r for r in reads if r >= 0)
            # an accessor reads one slot: its index's. A function reading several (the out
            # parameters' clearing, a pusher that clears them first, a helper reading a few
            # parameters through the accessors) is left to run.
            if len(rs) != 1 or rs[0] > len(locs):
                continue
            k = rs[0]
            loc = locs[k - 1] if k >= 1 else None   # slot 0: implicit index 0
            if loc:
                lv2 = {l2: 0 for l2 in locs}
                lv2[loc] = 3
                ok2, reads2, _ = self._run_probe(c, G, T, 12, lv2, 6)
                if sorted(r for r in reads2 if r >= 0) != [3]:
                    continue
            acc[c] = {"loc": loc}
        self.log("vm: %d accessors (read the one fake-stack slot their index names)" % len(acc))
        return acc

    def find_raise_switch(self, acc):
        """The VM state that makes a script error raise. A builtin's error ends in a test of
        a few globals (compiling, evaluating, a thread running): when none says a thread runs
        it returns a default, else it unwinds. Run the most called accessors with G = 0 (their
        error path), collect the globals that path reads, and set to 1 each one that turns
        the return into an unwind (the run no longer returns). Kept set in every later run."""
        emu, uc = self.emu, self.emu.uc
        data = [(self.im.base + s[1], self.im.base + s[1] + s[2]) for s in self.im.sections
                if not s[3] & 0x20000000]
        lo = min(a for a, _ in data)
        hi = max(b for _, b in data)
        if self.ctx_mode:
            lo, hi = min(lo, self.ctx), max(hi, self.ctx + 0x80000)
        self.switches = []
        ranked = sorted(acc, key=lambda c: -self.callee_freq.get(c, 0))[:6]
        for c in ranked:
            a = acc[c]
            lv = {a["loc"]: 1} if a["loc"] else {}
            reads = []

            def on_read(uc_, access, addr, size, value, data_):
                if addr not in (self.G, self.T) and size <= 4:
                    reads.append((addr, size))
            h = uc.hook_add(unicorn.UC_HOOK_MEM_READ, on_read, begin=lo, end=hi)
            uc.ctl_flush_tb()
            try:
                ok, _, _ = self._run_probe(c, self.G, self.T, 0, lv, 6)
            finally:
                uc.hook_del(h)
                uc.ctl_flush_tb()
            if not ok:
                continue  # this accessor's error path already unwinds
            self.tail_globals = set(g for g, _ in reads)
            for g, size in dict.fromkeys(reads):
                old = bytes(uc.mem_read(g, size))
                emu.patch(g, (1).to_bytes(size, "little"))
                ok2, _, _ = self._run_probe(c, self.G, self.T, 0, lv, 6)
                if not ok2:
                    self.switches.append((g, size))
                    break
                emu.patch(g, old)
            if self.switches:
                break
        self.log("vm: raise switch %s (set to 1: an accessor's error path unwinds instead of "
                 "returning a default)" % ([(hex(g), s) for g, s in self.switches] or "none needed"))
        return self.switches

    def type_acceptance(self, acc, types=range(0, 24)):
        """Per accessor, the value types it accepts: run once per type t (every dword of the
        fake stack = t), the path (block sequence) most types take is the mismatch path (raise,
        or return a default in the stripped builds), the types taking another path are
        accepted (a cast to string takes several). One path for every type, returning: a type
        query (Scr_GetType). The direct callees made on a mismatch and never on a match are the
        error raisers."""
        err = collections.Counter()
        for c, a in acc.items():
            lv = {a["loc"]: 1} if a["loc"] else {}
            k = 1 if a["loc"] else 0
            rets, direct, paths = [], {}, {}
            for t in types:
                ok, reads, d = self._run_probe(c, self.G, self.T, 12, lv, t, track=True)
                direct[t] = d
                paths[t] = self.last_path
                if ok:
                    rets.append(t)
            # the mismatch path is the one most types take; the accepted types take another
            mode = collections.Counter(paths.values()).most_common(1)[0][0]
            ok_types = [t for t in types if paths[t] != mode]
            a["accepts"] = ok_types
            a["query"] = not ok_types and len(rets) == len(list(types))
            if ok_types and len(ok_types) < 12:
                match = set(f for t in ok_types for f in direct[t])
                for t in types:
                    if t not in ok_types and direct[t] and direct[t][-1] not in match:
                        err[direct[t][-1]] += 1
                        break
        self.errors = [f for f, n in err.most_common(4) if n >= 3]
        self.log("vm: error raisers %s (the last direct call of an accessor on a type mismatch, "
                 "never made on a match; accessors: %s)" % (
                     [hex(f) for f in self.errors], [err[f] for f in self.errors]))
        # a "raiser" whose call is followed by another call in the accessor is the message
        # formatter (va), which the emulation cannot finish (the CRT is not initialised): it
        # is stubbed to return an empty string, and the raisers measured again
        fmts = [e for e in self.errors if self._followed_by_call(acc, e)]
        if fmts and not getattr(self, "formatters", None):
            self.formatters = fmts
            for f in fmts:
                self.emu.stub(f, self.zero_buf_addr())
            self.log("vm: formatter %s (its result feeds the next call): stubbed, raisers "
                     "measured again" % [hex(f) for f in fmts])
            return self.type_acceptance(acc, types)
        return acc

    def known_string(self):
        if not hasattr(self, "_known"):
            i = self.im.mem.find(b"\0classname\0")
            self._known = self.im.base + i + 1 if i >= 0 else self.zero_buf_addr()
        return self._known

    def zero_buf_addr(self):
        if not hasattr(self, "zero_buf"):
            self._stack(None)
        return self.zero_buf

    def _followed_by_call(self, acc, e):
        n = hits = 0
        for c in acc:
            # the whole accessor, past its early return: the error path follows it
            o = c - self.im.base
            ins = []
            for x in self.md.disasm(self.im.mem[o:o + 0x300], c):
                if x.mnemonic == "int3":
                    break
                ins.append(x)
            for i, x in enumerate(ins):
                if x.mnemonic == "call" and x.op_str == hex(e):
                    n += 1
                    # its result handed to the next call: `push eax` / `mov rcx, rax` before it
                    for y in ins[i + 1:i + 9]:
                        if y.mnemonic in ("call", "jmp"):
                            break
                        if (y.mnemonic == "push" and y.op_str == "eax") or (
                                y.mnemonic == "mov" and y.op_str.endswith((", rax", ", eax"))):
                            hits += 1
                            break
        return n and hits * 2 >= n

    def label(self, a):
        """Type label of an accessor from the types it accepts, by the VM's type names."""
        tn = self.type_names
        acc = set(a.get("accepts", []))
        nm = lambda t: tn.get(t, "t%d" % t)  # noqa: E731
        if a.get("query") or len(acc) >= 12:
            return "any"
        if 2 in acc and len(acc) > 1:
            return "string"      # a cast to string: string, and the types it converts
        if acc == {5, 6}:
            return "float"
        if acc == {2, 3}:
            return "string|istring"
        if len(acc) == 1:
            t = acc.pop()
            return {"object": "object", "localized string": "istring"}.get(nm(t), nm(t))
        if acc:
            return "|".join(sorted(nm(t).replace("localized string", "istring") for t in acc))
        lg = a.get("longest") or []
        if lg:
            return {"object": "entity", "localized string": "istring"}.get(nm(lg[0]), nm(lg[0]))
        return "?"

    # ---- step 5
    def setup_runner(self, acc):
        emu, uc = self.emu, self.emu.uc
        self.acc = acc
        self.run_state = None
        x64 = self.im.x64

        def acc_hook(uc_, addr, size, data):
            st = self.run_state
            if st is None:
                return
            a = acc[addr]
            loc = a["loc"]
            if loc is None:
                idx = 0
            elif loc in self._REG:
                idx = uc_.reg_read(self._REG[loc]) & 0xFFFFFFFF
            else:
                sp = uc_.reg_read(X.UC_X86_REG_ESP)
                idx = emu.r32(sp + (4 if loc == "s1" else 8))
            st["reads"].append((idx, addr))
            if idx >= st["N"]:
                st["stop"] = "param %d read with N=%d" % (idx, st["N"])
                uc_.emu_stop()
                return
            if self.alt_neutral == 3 and a.get("label") == "string":
                # mode 3: a string that names something real: the build's own "classname"
                # (a key every entity has: getentarray's second parameter)
                emu.do_return(self.known_string())
            elif self.alt_neutral in (1, 2) and a.get("numeric"):
                # mode 1: every number 1; mode 2: the first number 10, the others 0 (a time,
                # then accelerations that must not exceed it)
                st["nums"] = st.get("nums", 0) + 1
                v = 1 if self.alt_neutral == 1 else (10 if st["nums"] == 1 else 0)
                fbits = {1: 0x3F800000, 10: 0x41200000, 0: 0}[v]
                if self.im.x64:
                    uc_.reg_write(X.UC_X86_REG_XMM0, fbits if a.get("label") == "float" else v)
                emu.do_return(v)
            else:
                emu.do_return(a.get("neutral", 0))

        for a in acc:
            uc.hook_add(unicorn.UC_HOOK_CODE, acc_hook, begin=a, end=a)

        def err_hook(uc_, addr, size, data):
            st = self.run_state
            if st is None:
                return
            if self.strict:
                st["stop"] = "error raiser 0x%x" % addr
                uc_.emu_stop()
            else:
                st["errors"] += 1
                emu.do_return(0)
        for e in self.errors:
            uc.hook_add(unicorn.UC_HOOK_CODE, err_hook, begin=e, end=e)

        def g_read(uc_, access, addr, size, value, data):
            if self.run_state is not None:
                self.run_state["g_reads"] += 1
        uc.hook_add(unicorn.UC_HOOK_MEM_READ, g_read, begin=self.G, end=self.G + 3)

        # lenient, stripped builds: an error returns a default without calling a raiser; the
        # error tail's test of the raise switch is what counts it
        def sw_read(uc_, access, addr, size, value, data):
            if self.run_state is not None and not self.strict:
                self.run_state["errors"] += 1
        # (kept off: the switch is also read outside errors, which made counts unreliable)
        uc.ctl_flush_tb()

    def set_strict(self, strict):
        """strict: a script error ends the run (the raise switch set, the error raisers stop).
        lenient: errors return (the switch cleared, the raisers return 0), so a method whose
        object check fails on the zeroed entity 0 still goes on to read its parameters; a
        missing parameter is still caught at the accessor (index >= N)."""
        self.strict = strict
        for g, size in getattr(self, "switches", []):
            self.emu.patch(g, (1 if strict else 0).to_bytes(size, "little"))

    def run_handler(self, h, kind, N, count=200000):
        emu = self.emu
        emu.restore()
        base, top = self._stack(None)
        emu.w32(self.G, N)
        emu.wptr(self.T, top)
        st = {"N": N, "reads": [], "stop": None, "g_reads": 0, "errors": 0}
        emu.stop_reason = None
        self.run_state = st
        args = [0] if kind == "method" else []
        if self.ctx_mode:
            args = [self.ctx, 0] if kind == "method" else [self.ctx]
        try:
            emu.setup_call(h, args)
            emu.run(h, count=count)
            if emu.stop_reason and st["stop"] is None:
                st["stop"] = emu.stop_reason
            if not emu.returned and st["stop"] is None:
                st["stop"] = "instruction limit"
        except unicorn.UcError as e:
            if st["stop"] is None:
                st["stop"] = "%s at 0x%x" % (str(e).split(" (")[0], emu.uc.reg_read(emu.ip_reg))
        finally:
            self.run_state = None
        # a run cut by the instruction limit with no error is an engine loop (a lookup over
        # an empty table), not a script error: it counts as passing
        stop = st["stop"]
        got = {i for i, _ in st["reads"]}
        st["complete"] = stop is None
        # a run cut short by the emulation (a fault in engine code, the instruction limit, an
        # import) is not a script error: it passes when it had read every parameter given
        st["ok"] = stop is None or (
            N >= 1 and not stop.startswith(("error raiser", "param ", "errors"))
            and all(i in got for i in range(N)))
        return st

    def find_state_flags(self, h, kind, rounds=3, last=20):
        """A handler that raises before reading any parameter checks some state first (a
        precache outside level init, a method's object that is not a player). Run it with
        every data read recorded; among the last reads before the error, set each in turn
        (a pointer-sized read to a zeroed buffer, a smaller one to 1) until the run gets past
        the error. Up to `rounds` such fixes. [(address, size, bytes)]"""
        emu, uc = self.emu, self.emu.uc
        im = self.im
        lo = min(im.base + s[1] for s in im.sections if not s[3] & 0x20000000)
        hi = max(im.base + s[1] + s[2] for s in im.sections if not s[3] & 0x20000000)
        skip = ({self.G, self.T} | {g for g, _ in getattr(self, "switches", [])}
                | getattr(self, "tail_globals", set()))
        fixes = []
        for _ in range(rounds):
            reads = []

            def on_read(uc_, access, addr, size, value, data):
                if self.run_state is not None and not self.run_state["reads"] and addr not in skip:
                    reads.append((addr, size))
            hooks = [uc.hook_add(unicorn.UC_HOOK_MEM_READ, on_read, begin=lo, end=hi)]
            if self.ctx_mode:
                hooks.append(uc.hook_add(unicorn.UC_HOOK_MEM_READ, on_read, begin=self.ctx,
                                         end=self.ctx + 0x80000))
            uc.ctl_flush_tb()
            st = self.run_handler(h, kind, NMAX)
            for x in hooks:
                uc.hook_del(x)
            uc.ctl_flush_tb()
            if st["reads"] or not (st["stop"] or "").startswith(("error raiser", "instruction limit")):
                break
            # the reads just before the error, then the first reads of the handler (a loop in
            # the error path, an unwind through the CRT, floods the end with its own reads)
            uniq = list(dict.fromkeys(reads))
            cands = list(dict.fromkeys(uniq[:last] + uniq[-last:][::-1]))
            found = None
            for g, size in cands:
                if any(g == f[0] for f in fixes):
                    continue
                old = bytes(uc.mem_read(g, size))
                if size >= (8 if self.im.x64 else 4):
                    val = struct.pack("<Q" if self.im.x64 else "<I", self.zero_buf)
                else:
                    val = (1).to_bytes(size, "little")
                emu.patch(g, val)
                st2 = self.run_handler(h, kind, NMAX)
                emu.unpatch(g, old)
                if st2["reads"] or st2["stop"] is None:
                    found = (g, size, val, old)
                    break
            if not found:
                break
            fixes.append(found)
            emu.patch(found[0], found[2])
        for g, size, val, old in fixes:
            emu.unpatch(g, old)
        return fixes

    def measure(self, h, kind):
        """Strict runs first; when no N passes (an object check on the zeroed entity 0, an
        engine call the emulation cannot follow), the state the handler checks first is
        looked for and set; failing that, lenient runs."""
        self.set_strict(True)
        res = self._measure(h, kind)
        res["mode"] = "strict"
        if res["arity"] is None or not any(res["runs"][N][2] for N in res["runs"]):
            flags = self.find_state_flags(h, kind)
            if flags:
                for g, n, v, old in flags:
                    self.emu.patch(g, v)
                r1 = self._measure(h, kind)
                for g, n, v, old in flags:
                    self.emu.unpatch(g, old)
                if r1["arity"] is not None and any(r1["runs"][N][2] for N in r1["runs"]):
                    r1["mode"] = "strict+state"
                    r1["state"] = [hex(g) for g, n, v, old in flags]
                    return r1
        if res["arity"] is None or not any(res["runs"][N][2] for N in res["runs"]):
            self.set_strict(False)
            r2 = self._measure(h, kind)
            self.set_strict(True)
            if r2["arity"] is not None:
                r2["mode"] = "lenient"
                r2["strict_stop"] = res["runs"][NMAX][1]
                return r2
        return res

    def _measure(self, h, kind):
        # N = NMAX first, then N = 0.. up to two past the highest index any run read (and at
        # least 0..4): the counts in between behave as NMAX does (checked by the max_checked
        # and variadic tests, which look at NMAX and at the counts run)
        runs = {NMAX: self.run_handler(h, kind, NMAX)}
        hi = max([i for i, _ in runs[NMAX]["reads"]], default=-1)
        N = 0
        while N < NMAX and (N <= max(4, hi + 2)):
            runs[N] = self.run_handler(h, kind, N)
            hi = max([hi] + [i for i, _ in runs[N]["reads"] if i < N])
            N += 1
        # a run failing on an error after reading its parameters may be a value check on the
        # neutral 0 (a speed that must be > 0): such counts are run again with 1 / 1.0
        for N, r in list(runs.items()):
            if (not r["ok"] and (r["stop"] or "").startswith("error raiser") and r["reads"]
                    and all(i < N for i, _ in r["reads"])):
                for mode in (1, 2, 3):
                    self.alt_neutral = mode
                    r2 = self.run_handler(h, kind, N)
                    self.alt_neutral = False
                    if r2["ok"]:
                        r2["alt"] = mode
                        runs[N] = r2
                        break
        # lenient: the errors every count raises (an object or state check) are the baseline;
        # a count raising more (a usage check) fails
        if not self.strict:
            fin = [r["errors"] for r in runs.values() if r["ok"]]
            base = min(fin) if fin else 0
            for r in runs.values():
                if r["ok"] and r["errors"] > base:
                    r["ok"] = False
                    r["stop"] = "%d errors raised (baseline %d)" % (r["errors"], base)
        oks = sorted(N for N in runs if runs[N]["ok"])
        res = {"runs": {N: (r["ok"], r["stop"], sorted({i for i, _ in r["reads"]}), r["g_reads"],
                            r["errors"]) for N, r in runs.items()}}
        if not oks:
            res["arity"] = None
            return res
        mn = min(oks)
        mx_read = max([max([i for i, _ in runs[N]["reads"]], default=-1) for N in oks])
        mx = max(mn, mx_read + 1)
        variadic = NMAX in oks and any(i == NMAX - 1 for i, _ in runs[NMAX]["reads"])
        checked = any(not runs[N]["ok"] for N in runs if N > mx)
        # types per index, from the ok run with the most parameters read
        types = {}
        for N in oks:
            for i, a in runs[N]["reads"]:
                types.setdefault(i, set()).add(a)
        res.update(arity=(mn, mx), variadic=variadic, max_checked=checked,
                   types={i: sorted(v) for i, v in types.items()},
                   g_reads=max(runs[N]["g_reads"] for N in oks),
                   holes=[N for N in range(mn, mx + 1) if N in runs and N not in oks])
        return res
