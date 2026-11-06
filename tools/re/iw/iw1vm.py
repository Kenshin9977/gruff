"""CoD1 (IW1) arity: the game module calls the engine's script VM through a table of function
pointers the engine fills at load (all zero in gamex86.dll). The slots are told apart by how
the game code calls them, statically:

- the parameter count (Scr_GetNumParam): the slot whose call result is compared with a small
  constant right after the call, most often;
- the accessors (Scr_GetInt/Float/String/Vector/Entity...): slots called right after a
  `push imm8` (a parameter index), many times;
- the script error (Scr_Error): the slot called with a string that starts "USAGE" (the game's
  own usage messages) or the most common one taking a string literal on error paths.

Each slot then points at a stub: the count returns N, an accessor records the index on the stack
(an index >= N fails the run), the error slot fails the run, any other slot returns 0. Handler
runs for N = 0..16 as for the other games; no parameter type (the accessors' code is in the
packed engine, only their slot is known).
"""
import collections
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from image import user_site  # noqa: E402
from iwemu import Emu  # noqa: E402

user_site()
import capstone  # noqa: E402
import unicorn  # noqa: E402
from unicorn import x86_const as X  # noqa: E402

NMAX = 16


def slots(im):
    """Indirect calls `call dword ptr [abs]` whose slot is zero in the file: engine pointers.
    {slot: [(call va, previous instructions)]}"""
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    out = collections.defaultdict(list)
    for name, sva, vsz, ch in im.sections:
        if not ch & 0x20000000:
            continue
        m = im.mem
        for o in range(sva, sva + vsz - 6):
            if m[o] == 0xFF and m[o + 1] == 0x15:
                s = struct.unpack_from("<I", m, o + 2)[0]
                if im.in_image(s) and not im.is_code(s) and im.u32(s) == 0:
                    out[s].append(im.base + o)
    return out


def classify(im, sl, log):
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    cmp_after = collections.Counter()
    push_idx = collections.Counter()
    usage = collections.Counter()
    for s, calls in sl.items():
        for c in calls:
            o = c - im.base
            after = list(md.disasm(im.mem[o:o + 24], c))[1:4]
            if any(i.mnemonic == "cmp" and i.op_str.startswith("eax, ") for i in after[:2]):
                cmp_after[s] += 1
            # the instruction just before the call
            for back in range(2, 8):
                ins = list(md.disasm(im.mem[o - back:o], c - back))
                if ins and ins[-1].address + ins[-1].size == c:
                    p = ins[-1]
                    if p.mnemonic == "push" and (p.op_str.startswith("0x") or p.op_str.isdigit()):
                        try:
                            v = int(p.op_str, 16) if p.op_str.startswith("0x") else int(p.op_str)
                        except ValueError:
                            break
                        if v < 16:
                            push_idx[s] += 1
                        elif im.in_image(v):
                            t = im.cstr(v, 120) or b""
                            if t.upper().startswith(b"USAGE"):
                                usage[s] += 1
                    break
    nump = cmp_after.most_common(1)[0][0] if cmp_after else None
    acc = [s for s, n in push_idx.items() if n >= 5 and s != nump]
    err = usage.most_common(1)[0][0] if usage else None
    log("iw1 vm: %d engine slots; count slot 0x%x (compared after %d calls); %d accessor slots "
        "(called after `push index`); error slot %s (%d USAGE messages)" % (
            len(sl), nump or 0, cmp_after.get(nump, 0), len(acc), hex(err) if err else None,
            usage.get(err, 0)))
    return nump, acc, err


class IW1VM:
    def __init__(self, im, log=print):
        self.im = im
        self.emu = Emu(im)
        self.emu.install_import_stubs()
        sl = slots(im)
        self.nump, self.acc, self.err = classify(im, sl, log)
        uc = self.emu.uc
        base = 0xE8000000
        uc.mem_map(base, 0x10000)
        uc.mem_write(base, b"\xf4" * 0x10000)
        self.stub_of = {}
        for i, s in enumerate(sorted(sl)):
            st = base + 16 * i
            self.stub_of[st] = s
            self.emu.patch(s, struct.pack("<I", st))
        self.state = None
        acc = set(self.acc)

        def on_stub(uc_, addr, size, data):
            s = self.stub_of.get(addr)
            st = self.state
            if st is None:
                self.emu.do_return(0)
                return
            if s == self.nump:
                self.emu.do_return(st["N"])
            elif s in acc:
                sp = uc_.reg_read(X.UC_X86_REG_ESP)
                idx = self.emu.r32(sp + 4)
                st["reads"].append((idx, s))
                if idx >= st["N"]:
                    st["stop"] = "param %d read with N=%d" % (idx, st["N"])
                    uc_.emu_stop()
                    return
                self.emu.do_return(0)
            elif s == self.err:
                st["stop"] = "error slot"
                uc_.emu_stop()
            else:
                self.emu.do_return(0)
        uc.hook_add(unicorn.UC_HOOK_CODE, on_stub, begin=base, end=base + 0x10000)
        self.emu.heap_mark = self.emu.heap_next
        uc.ctl_flush_tb()

    def run(self, h, kind, N):
        emu = self.emu
        emu.restore()
        st = {"N": N, "reads": [], "stop": None}
        self.state = st
        try:
            emu.setup_call(h, [0] if kind == "method" else [])
            emu.run(h, count=200000)
            if not emu.returned and st["stop"] is None:
                st["stop"] = emu.stop_reason or "instruction limit"
        except unicorn.UcError as e:
            if st["stop"] is None:
                st["stop"] = str(e).split(" (")[0]
        self.state = None
        got = {i for i, _ in st["reads"]}
        stop = st["stop"]
        st["ok"] = stop is None or (N >= 1 and not stop.startswith(("error", "param"))
                                    and all(i in got for i in range(N)))
        return st

    def measure(self, h, kind):
        runs = {N: self.run(h, kind, N) for N in range(NMAX + 1)}
        oks = [N for N in runs if runs[N]["ok"]]
        res = {"runs": {N: (r["ok"], r["stop"], sorted({i for i, _ in r["reads"]}), 0, 0)
                        for N, r in runs.items()}, "mode": "strict", "types": {}}
        if not oks:
            res["arity"] = None
            return res
        mn = min(oks)
        mx = max(mn, max([max([i for i, _ in runs[N]["reads"]], default=-1) for N in oks]) + 1)
        res.update(arity=(mn, mx), variadic=NMAX in oks and any(i == NMAX - 1 for i, _ in runs[NMAX]["reads"]),
                   max_checked=any(not runs[N]["ok"] for N in runs if N > mx), g_reads=0,
                   holes=[N for N in range(mn, mx + 1) if N not in oks])
        return res
