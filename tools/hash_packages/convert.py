"""Convert existing name dictionaries into gruff hash packages (docs/hash-packages.md).

Every input is read, never modified; packages are written to --out (default: the
`hash-packages/` folder beside gruff, outside every repository). Name lists recovered
from a game's files are Activision-derived: keep the packages local, never commit them.

    python -I convert.py bo7-archive <names/all_names.tsv>        BO7 pairs, one section per family
    python -I convert.py wni <file.wni> --game t11                 GrayWolf / Wraith name index
    python -I convert.py csv <file.csv> --game t8,t9 [--family f]  hash,name lines
    python -I convert.py scripts <dir> --game '*'                  clear identifiers of a script tree
    python -I convert.py all [--config sources.local.json]         every source of the config

Stdlib only.
"""

import argparse
import json
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_OUT = os.path.normpath(os.path.join(HERE, "..", "..", "..", "hash-packages"))
DEFAULT_CONFIG = os.path.join(HERE, "sources.local.json")

# bo7-gsc-archive family column -> gsc_hash family id.
BO7_FAMILIES = {
    "function": "t10.function",
    "var": "t10.var",
    "namespace": "t10.namespace",
    "event": "t10.event",
    "bundlevar": "t10.bundlevar",
    "dvarhash": "t10.dvar",
    "xhash": "t10.xhash",
    "case": "t10.case",
    "assetlit": "t10.assetlit",
    "script": "t10.script",
    "bundlekey": "t10.bundlekey",
}


def header(games, source, licence, family=None, extra=()):
    lines = ["# gsc-hash-package: 1", "# game: " + (", ".join(games) if games else "*")]
    if family:
        lines.append("# family: " + family)
    lines.append("# source: " + source.replace("\n", " "))
    lines.append("# licence: " + licence.replace("\n", " "))
    for k, v in extra:
        lines.append("# %s: %s" % (k, str(v).replace("\n", " ")))
    return "\n".join(lines) + "\n"


def clean(name):
    """A name a package line can hold (no tab, no line break, not starting with '#')."""
    if not name or "\t" in name or "\n" in name or "\r" in name or name.startswith("#"):
        return None
    return name


def write(out_dir, file_name, text, count):
    os.makedirs(out_dir, exist_ok=True)
    path = os.path.join(out_dir, file_name)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print("%s: %d entries" % (path, count))
    return path


def parse_hex(s):
    s = s.strip()
    if s.lower().startswith("0x"):
        s = s[2:]
    if "_" in s:
        s = s.rsplit("_", 1)[1]
    if not s or len(s) > 16:
        return None
    try:
        return int(s, 16)
    except ValueError:
        return None


def bo7_archive(path, out_dir, games, source, licence, name="t11-bo7-gsc-archive.tsv", names_only=False):
    sections = {}
    with open(path, encoding="utf-8", errors="surrogateescape") as f:
        for i, line in enumerate(f):
            cols = line.rstrip("\r\n").split("\t")
            if i == 0 or len(cols) < 5:
                continue
            fam = BO7_FAMILIES.get(cols[0])
            n = clean(cols[4])
            v = parse_hex(cols[3])
            if fam and n and v is not None:
                sections.setdefault(fam, []).append((v, n))
    if names_only:
        names = sorted({n for pairs in sections.values() for _, n in pairs})
        text = header(games, source, licence, extra=[("kind", "plain names, hashed by the toolchain")])
        return write(out_dir, name, text + "\n".join(names) + "\n", len(names))
    text = header(games, source, licence, extra=[("families", len(sections))])
    count = 0
    for fam in sorted(sections):
        text += "# family: %s\n" % fam
        for v, n in sorted(set(sections[fam])):
            text += "%x\t%s\n" % (v, n)
            count += 1
    return write(out_dir, name, text, count)


def lz4_block(src, size):
    out = bytearray()
    i = 0
    n = len(src)
    while i < n:
        token = src[i]
        i += 1
        lit = token >> 4
        if lit == 15:
            while True:
                b = src[i]
                i += 1
                lit += b
                if b != 255:
                    break
        out += src[i:i + lit]
        i += lit
        if i >= n:
            break
        off = src[i] | (src[i + 1] << 8)
        i += 2
        ml = token & 15
        if ml == 15:
            while True:
                b = src[i]
                i += 1
                ml += b
                if b != 255:
                    break
        ml += 4
        start = len(out) - off
        if off == 0 or start < 0:
            raise ValueError("bad LZ4 offset")
        for k in range(ml):
            out.append(out[start + k])
    if len(out) != size:
        raise ValueError("LZ4 size mismatch")
    return bytes(out)


def read_wni(path):
    data = open(path, "rb").read()
    magic, _version, count, packed, unpacked = struct.unpack_from("<IHIII", data, 0)
    if magic != 0x20494E57:
        raise ValueError("%s: not a WNI file" % path)
    raw = lz4_block(data[18:18 + packed], unpacked)
    at = 0
    out = []
    for _ in range(count):
        key = struct.unpack_from("<Q", raw, at)[0]
        at += 8
        end = raw.index(b"\0", at)
        out.append((key, raw[at:end].decode("utf-8", "replace")))
        at = end + 1
    return out


def pairs_package(pairs, out_dir, name, games, source, licence, family=None, names_only=False):
    text = header(games, source, licence, family=family)
    seen = set()
    count = 0
    lines = []
    for v, n in pairs:
        n = clean(n)
        if n is None:
            continue
        key = n if names_only else (v, n)
        if key in seen:
            continue
        seen.add(key)
        lines.append(n if names_only else "%x\t%s" % (v, n))
        count += 1
    lines.sort()
    return write(out_dir, name, text + "\n".join(lines) + ("\n" if lines else ""), count)


def read_csv(path):
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.rstrip("\r\n")
            if "," not in line:
                continue
            h, n = line.split(",", 1)
            v = parse_hex(h)
            if v is not None:
                out.append((v, n))
    return out


IDENT = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")
STRING = re.compile(r'[#@]?"((?:[^"\\\n]|\\.)*)"')
HASHED = re.compile(r"^(function|var|namespace|class|event|hash|script|x_fn|x_fld|x_evt|x_key)_")
KEYWORDS = set(
    "if else for foreach while do switch case default break continue return wait thread "
    "childthread function class var const in new true false undefined self level game anim "
    "notify endon waittill isdefined".split()
)


def script_names(root):
    names = set()
    for dp, _, fs in os.walk(root):
        for f in fs:
            if not f.lower().endswith((".gsc", ".csc", ".gsh")):
                continue
            text = open(os.path.join(dp, f), encoding="utf-8", errors="replace").read()
            text = re.sub(r"//[^\n]*", "", text)
            for m in STRING.finditer(text):
                s = m.group(1)
                if s and len(s) <= 128 and not HASHED.match(s) and re.fullmatch(r"[\w/\\.:-]+", s):
                    names.add(s.lower())
            for m in IDENT.finditer(STRING.sub(" ", text)):
                w = m.group(0).lower()
                if w not in KEYWORDS and not HASHED.match(w) and len(w) > 1:
                    names.add(w)
            rel = os.path.relpath(os.path.join(dp, f), root).replace(os.sep, "/").lower()
            names.add(rel)
            names.add(rel.rsplit(".", 1)[0])
    return sorted(names)


def scripts_package(root, out_dir, name, games, source, licence):
    names = script_names(root)
    text = header(games, source, licence, extra=[("kind", "plain names, hashed by the toolchain")])
    return write(out_dir, name, text + "\n".join(names) + "\n", len(names))


def run_entry(e, out_dir):
    kind = e["kind"]
    games = [g.strip() for g in e.get("game", "*").split(",") if g.strip() and g.strip() != "*"]
    src = e.get("source", e["path"])
    lic = e.get("licence", "unknown: keep local")
    name = e.get("name")
    if kind == "bo7-archive":
        return bo7_archive(e["path"], out_dir, games, src, lic, name or "t11-bo7-gsc-archive.tsv",
                           e.get("names_only", False))
    if kind == "wni":
        stem = os.path.splitext(os.path.basename(e["path"]))[0]
        return pairs_package(read_wni(e["path"]), out_dir, name or stem + ".tsv", games, src, lic,
                             e.get("family"), e.get("names_only", False))
    if kind == "csv":
        stem = os.path.splitext(os.path.basename(e["path"]))[0]
        return pairs_package(read_csv(e["path"]), out_dir, name or stem + ".tsv", games, src, lic,
                             e.get("family"), e.get("names_only", False))
    if kind == "scripts":
        return scripts_package(e["path"], out_dir, name or "script-names.tsv", games, src, lic)
    raise SystemExit("unknown kind %r" % kind)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("kind", choices=["bo7-archive", "wni", "csv", "scripts", "all"])
    ap.add_argument("path", nargs="?")
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--game", default="*", help="profile ids, comma-separated, or *")
    ap.add_argument("--family", help="the family of every pair (default: verified per family on load)")
    ap.add_argument("--names-only", action="store_true", help="write the names without their hashes")
    ap.add_argument("--name", help="output file name")
    ap.add_argument("--source", help="the header's source line (default: the input path)")
    ap.add_argument("--licence", default="unknown: keep local", help="the header's licence line")
    ap.add_argument("--config", default=DEFAULT_CONFIG, help="for `all`: the sources file")
    a = ap.parse_args(argv)
    if a.kind == "all":
        with open(a.config, encoding="utf-8") as f:
            cfg = json.load(f)
        out = cfg.get("out", a.out)
        for e in cfg["sources"]:
            if not os.path.exists(e["path"]):
                print("skip (missing): %s" % e["path"], file=sys.stderr)
                continue
            run_entry(e, out)
        return 0
    if not a.path:
        ap.error("a path is required")
    e = {"kind": a.kind, "path": a.path, "game": a.game, "source": a.source or a.path,
         "licence": a.licence, "names_only": a.names_only}
    if a.family:
        e["family"] = a.family
    if a.name:
        e["name"] = a.name
    run_entry(e, a.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
