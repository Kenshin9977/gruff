"""Names for IW4 (the MW2 x64 rebuild keeps neither names nor ids): each IW4 handler matched to
the IW5 handler with the same code shape. Both rebuilds come from one port of sibling engines,
and most GScr handlers kept their code. The shape: the handler's instructions decoded
linearly to its first ret (at most 96), mnemonic only, a direct call written as the accessor's
type label when it calls an accessor and as `call` otherwise, then its measured arity. A name
is taken only from a unique match of the same kind: the inference is recorded as such
(name_source = "iw5-handler-match"), and the name itself is gsc-tool's (GPL-3.0).

    py -3.11 -I match.py        -> out/iw4_names.json
"""
import collections
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from image import Image, user_site  # noqa: E402
import gsctool  # noqa: E402

user_site()
import capstone  # noqa: E402


def shapes(doc):
    im = Image(doc["exe"])
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    acc = {int(k, 16): v["label"] for k, v in doc["vm"]["accessors"].items()}
    out = {}
    for e in doc["entries"]:
        h = int(e["handler"], 16)
        o = h - im.base
        sig = []
        for ins in md.disasm(im.mem[o:o + 0x600], h):
            if ins.mnemonic in ("call", "jmp") and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                sig.append("A:" + acc[t] if t in acc else ins.mnemonic)
            else:
                sig.append(ins.mnemonic)
            if ins.mnemonic in ("ret", "int3") or len(sig) >= 96:
                break
        r = doc["runs"].get("%x:%s" % (h, e["kind"] or "function"), {})
        out[(e.get("index", e.get("id")), e["kind"])] = (tuple(sig), tuple(r.get("arity") or ()))
    return out


def main():
    d4 = json.load(open(os.path.join(HERE, "out", "iw4.json")))
    s4 = shapes(d4)
    s5 = {}
    for g in ("iw5", "iw5_sp"):
        p = os.path.join(HERE, "out", g + ".json")
        if os.path.exists(p):
            for k, v in shapes(json.load(open(p))).items():
                s5.setdefault(k, v)
    fn, me = gsctool.names("iw5_pc")
    by_shape = collections.defaultdict(set)
    for (i, kind), sh in s5.items():
        nm = (me if kind == "method" else fn).get(i)
        if nm:
            by_shape[(kind, sh)].add(nm.lower())
    # an IW4 shape seen once in IW4 and naming one IW5 name: matched
    count4 = collections.Counter((k, sh) for (i, k), sh in s4.items())
    names = {}
    for (i, kind), sh in s4.items():
        cands = by_shape.get((kind, sh), set())
        if len(cands) == 1 and count4[(kind, sh)] == 1:
            names["%d:%s" % (i, kind)] = cands.pop()
    used = collections.Counter(names.values())
    names = {k: v for k, v in names.items() if used[v] == 1}
    out = os.path.join(HERE, "out", "iw4_names.json")
    json.dump(names, open(out, "w"), indent=0, sort_keys=True)
    print("IW4 handlers %d; named by a unique IW5 shape match: %d -> %s" % (len(s4), len(names), out))


if __name__ == "__main__":
    main()
