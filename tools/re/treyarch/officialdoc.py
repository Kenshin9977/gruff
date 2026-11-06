"""Parse the BO3 mod-tools script API reference (docs_modtools/bo3_scriptapifunctions.htm), read
locally and read-only. Only facts are taken: the name, whether it is called on an entity, the
mandatory and optional parameter counts, CLIENT/SERVER. No description text leaves this module:
the result is used as an oracle and nothing from it is written to the repository but counts and
names that the executable already gives."""
import html
import re
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common import paths  # noqa: E402

# the mod tools install: bo3_root in tools/re/paths.local.json, or GSC_RE_BO3_ROOT
BO3 = paths.get("bo3_root") or r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III"

DEFAULT = os.path.join(BO3, "docs_modtools", "bo3_scriptapifunctions.htm")

H1 = re.compile(r"<H1>(.*?)</H1>(.*?)(?=<HR>|\Z)", re.S | re.I)


def parse(path=DEFAULT):
    with open(path, "rb") as f:
        text = f.read().decode("latin1")
    out = []
    for m in H1.finditer(text):
        head, body = m.group(1), m.group(2)
        plain = html.unescape(re.sub(r"<[^>]+>", "", head)).strip()
        # "<ret> [<entity>] Name(<a>,[b])"
        mm = re.match(r"^(\S+)\s+(?:(<[^>]*>)\s+)?([A-Za-z_][\w]*)\s*\((.*)\)\s*$", plain, re.S)
        if not mm:
            out.append({"raw": plain, "name": None})
            continue
        ret, ent, name, params = mm.groups()
        req = len(re.findall(r"\[MANDATORY\]", body))
        opt = len(re.findall(r"\[OPTIONAL\]", body))
        # head-only counts as a cross-check: <x> mandatory, [x] optional
        hreq = len(re.findall(r"<I>&lt;", head))
        hopt = len(re.findall(r"<I>\[", head))
        cs = re.search(r"CLIENT/SERVER:\s*</B>\s*([^<]*)", body)
        side = cs.group(1).strip() if cs else ""
        out.append({
            "name": name,
            "method": ent is not None,
            "entity": ent.strip("<>") if ent else None,
            "req": req, "opt": opt, "head_req": hreq, "head_opt": hopt,
            "side": side,
        })
    return out


if __name__ == "__main__":
    import collections
    import sys
    e = parse(sys.argv[1] if len(sys.argv) > 1 else DEFAULT)
    print(len(e), "entries,", sum(1 for x in e if not x["name"]), "unparsed")
    print(collections.Counter(x.get("side") for x in e))
    print(sum(1 for x in e if x.get("method")), "methods")
    bad = [x for x in e if x["name"] and (x["req"], x["opt"]) != (x["head_req"], x["head_opt"])]
    print(len(bad), "body/head count mismatches")
    for x in bad[:10]:
        print(" ", x["name"], x["req"], x["opt"], x["head_req"], x["head_opt"])
    for x in [x for x in e if not x["name"]][:10]:
        print(" unparsed:", x["raw"][:100])
