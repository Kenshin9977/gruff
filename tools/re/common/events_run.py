"""The events extraction driver shared by treyarch/events.py and iw/events.py: a profile in,
out/events_<game>.json out (then events_build.build). See common/events.py for the method.

Profile keys: title, exe_key, extractor; core (rva), core_args {inst, obj, event, count}
(an int is that argument, a tuple a spec such as ("deref", 3), None absent); hash,
hash_family, hash_mask, hash_digits (hashed engines; hash None for string-id engines);
require_cstr; const_init [rva], const_alloc rva, const_width 4|2 (scr_const); vmpub_top rva,
vmpub_stride, value_size, type_off, top_disp, top_width (pushers); entity_sources [rva],
object_sources {rva: label}; payload "struct" (T8+); waittill_any; name_lists; corpus [dirs];
vm_of_inst {inst: "gsc"|"csc"}.
"""
import argparse
import json
import os

from .image import Image
from . import paths
from . import events as E
from . import events_sites as S
from . import events_build as B

RE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(RE, "out")


def extract(game, prof, exe, out_path):
    img = Image(exe)
    log = [img.describe()]
    ci = E.CallIndex(img)
    dec = E.Decoder(img, ci)
    probe = E.Probe(img, dec, prof)
    # 1. string constants
    strs = []
    sets = []
    if prof.get("const_init"):
        sets.append((prof["const_init"], prof["const_alloc"], prof.get("const_str_arg", 0)))
    sets += prof.get("const_extra", [])  # another init with its own allocator (T5's client)
    for inits, alloc, sarg in sets:
        slots, sizes, these, errs = E.const_slots(
            img, [img.base + r for r in inits], img.base + alloc,
            width=prof["const_width"], str_arg=sarg, log=log,
            out_arg=prof.get("const_out_arg"),
            decrypt=(img.base + prof["const_decrypt"]) if prof.get("const_decrypt") else None)
        if len(slots) < 0.95 * len(these):
            raise SystemExit("scr_const: %d slots for %d strings" % (len(slots), len(these)))
        w = prof["const_width"]
        base = E.TOKEN_BASE if w == 4 else E.TOKEN16
        off = len(strs)
        strs += these
        for a, st in slots.items():
            # the slot holds the token of its string (the first allocator call returning it)
            probe.uc.mem_write(a, (base + off + these.index(st)).to_bytes(w, "little"))
    # 2. notifiers
    ca = prof["core_args"]
    spec = lambda v: None if v is None else (v if isinstance(v, tuple) else ("arg", v))  # noqa
    core = E.Notifier(img.base + prof["core"], "core", spec(ca["inst"]), spec(ca["obj"]),
                      spec(ca["event"]), spec(ca.get("count")), "id", prof.get("core_recv"), None,
                      "profile anchor")
    notifiers, ent_res = E.discover_wrappers(img, ci, dec, probe, core, prof["hash"], log)
    for rva, xa in prof.get("extra_cores", ()):
        # a second notify path into the VM that does not go through the first core (BO6)
        x = E.Notifier(img.base + rva, "core", spec(xa["inst"]), spec(xa["obj"]),
                       spec(xa["event"]), spec(xa.get("count")), "id", None, None,
                       "profile anchor")
        more, er = E.discover_wrappers(img, ci, dec, probe, x, prof["hash"], log)
        have = {n.va for n in notifiers}
        notifiers += [n for n in more if n.va not in have]
        ent_res.update(er)
    kinds = {n.event_kind for n in notifiers}
    if prof.get("require_cstr", True) and "cstr" not in kinds:
        raise SystemExit("core %#x: no wrapper hashes a C string into its event: the event "
                         "parameter is not shown to be a %s hash" % (prof["core"],
                                                                     prof["hash_family"]))
    log.append("notifiers: %d (core + %d wrappers); entity-id resolvers %s" % (
        len(notifiers), len(notifiers) - 1, [hex(a - img.base) for a in ent_res]))
    wrapper_fns = {n.va for n in notifiers}
    # 3. pushers
    pushers = None
    if prof.get("vmpub_top") is not None or prof.get("vmpub_ctx"):
        tn = E.type_name_table(img)
        if not tn and prof.get("typenames"):  # the build carries no table: a sibling's
            tn = list(prof["typenames"])
            log.append("type names: none in this build; the profile's (%s)" % prof["typenames_from"])
        log.append("type names: %s" % tn[:12])
        if not tn or tn[0] != "undefined":
            raise SystemExit("type-name table not found")
        if prof.get("vmpub_ctx"):
            # the top behind a pointer in each VM context (T11: [[ctx + 0x688]]): one cell per
            # context, its address written into the context; an "inst" is the context's rva
            vc = prof["vmpub_ctx"]
            cells = {}
            for c in vc["ctxs"]:
                cells[c] = probe.emu.alloc(0x10)
                probe.emu.wptr(img.base + c + vc["top_off"], cells[c])
            top = lambda inst: cells[inst]  # noqa
            log.append("stack top cells: %s" % {hex(c): hex(v) for c, v in cells.items()})
        else:
            top = lambda inst: img.base + prof["vmpub_top"] + inst * prof["vmpub_stride"]  # noqa
        cores = S.PushCores(img, prof["top_disp"], prof["value_size"], prof["type_off"],
                            prof.get("top_width", "qword"),
                            (img.base + prof["vmpub_top"]) if prof.get("top_abs") else None,
                            prof.get("tag_width", "dword"), prof.get("top_store", False))
        ent_fns = set(ent_res) | {img.base + r for r in prof.get("entity_sources", ())}
        pushers = S.Pushers(img, probe, cores, top, prof["value_size"], prof["type_off"], tn,
                            entity_fns=ent_fns, insts=prof.get("insts", (0, 1)),
                            tag_width=prof.get("tag_width", "dword"))
    # 3b. wrappers that push their own payload (a fixed count: G_Damage-like callers of
    # Scr_Notify): the pushes their probe run makes before the parent's entry
    inner = {}
    if pushers is not None:
        by_va = {n.va: n for n in notifiers}
        for n in notifiers:
            if not (n.count and n.count[0] == "const" and n.count[1] > 0 and n.parent in by_va):
                continue
            fstart, insns, idx = dec.at(n.va)
            if insns is None:
                continue
            g = probe.probe_wrapper(fstart, insns, by_va[n.parent])
            if g is None:
                continue
            ps = []
            for k, (addr, tgt, args) in enumerate(g["skipped"]):
                c = pushers.classify(tgt, args)
                if c is not None and c[2] is None and k and \
                        g["skipped"][k - 1][1] in pushers.entity_fns:
                    c = (c[0], True, g["skipped"][k - 1][1])
                if c is not None:
                    ps.append({"at": addr, "fn": tgt, "vm_type": c[0], "entity": c[1],
                               "source": c[2], "type": S.schema_type(c[0], c[1])})
            inner[n.va] = ps
            log.append("wrapper %#x pushes %d of its %d values itself: %s" % (
                n.va - img.base, len(ps), n.count[1], [p["vm_type"] for p in ps]))
    # 4. sites
    res = S.SiteResolver(img, dec, probe, notifiers, pushers)
    sites = []
    for n in notifiers:
        for site, kind in ci.callers(n.va):
            fstart, insns, idx = dec.at(site)
            if fstart in wrapper_fns:
                continue
            r = res.resolve(site, n)
            r["call_kind"] = kind
            if n.va in inner and "fail" not in r and not r.get("pushes"):
                r["pushes"] = inner[n.va]
                r["pushes_from"] = "wrapper"
            sites.append(r)
    log.append("sites: %d, resolved %d" % (len(sites), sum(1 for s in sites if "fail" not in s)))
    mask = prof.get("hash_mask", 0xFFFFFFFF)
    wanted = set()
    for s in sites:
        for v in [s.get("event")] + s.get("alternatives", []):
            if v is not None:
                wanted.add(v & mask)
    names = E.image_names(img, wanted, prof["hash"], mask) if prof.get("hash") else {}
    log.append("image strings naming %d of %d event values" % (len(names), len(wanted)))
    out = {"game": game, "exe": os.path.basename(exe), "sha256": B.sha256(exe),
           "pe_timestamp": img.timestamp, "image_base": img.base,
           "notifiers": [n.as_dict(img.base) for n in notifiers],
           "const_strings": strs, "image_names": {str(h): v for h, v in names.items()},
           "sites": [B.rebase(s, img.base) for s in sites], "log": log}
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1)
    print("\n".join(log))


def main(profiles):
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["extract", "build"])
    ap.add_argument("--game", required=True, choices=sorted(profiles))
    ap.add_argument("--exe")
    a = ap.parse_args()
    prof = profiles[a.game]
    out = os.path.join(OUT, "events_%s.json" % a.game)
    if a.cmd == "build" and prof.get("merge_into"):
        raise SystemExit("%s is merged into %s: extract it, then build --game %s" % (
            a.game, prof["merge_into"], prof["merge_into"]))
    if a.cmd == "extract":
        extract(a.game, prof, a.exe or paths.need(prof["exe_key"]), out)
    else:
        B.build(a.game, prof, out)
