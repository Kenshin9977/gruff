"""Naming hashes: a hash is named only by a string whose hash (by the engine's family) equals it.
Candidate strings come from sources tried in order; the first source that names a hash wins and
is recorded as its name_source. Nothing but the string is kept from a source.

Candidates may come with a claimed hash (a hash list: "hash,name"); the claim is ignored, the
string is hashed again.
"""
import csv
import os
import re

IDENT = re.compile(r"^[a-z_][a-z0-9_]*$")


def name_hashes(hashes, sources, hash_fn, mask=None):
    """hashes: set of ints. sources: [(label, iterable of candidate strings)]. Returns
    ({hash: name}, {hash: label}). Only identifiers ([a-z_][a-z0-9_]*, lower-cased) are kept."""
    names, src = {}, {}
    for label, cands in sources:
        for s in cands:
            n = s.strip().lower()
            if not IDENT.match(n):
                continue
            h = hash_fn(n)
            if mask is not None:
                h &= mask
            if h in hashes and h not in names:
                names[h], src[h] = n, label
    return names, src


def csv_column(path, col=1, delimiter=",", skip_header=True):
    """The strings of one column of a CSV/TSV file (missing file: nothing)."""
    if not os.path.exists(path):
        return
    with open(path, encoding="utf-8", errors="replace", newline="") as f:
        r = csv.reader(f, delimiter=delimiter)
        for i, row in enumerate(r):
            if i == 0 and skip_header and row and not row[0].lower().startswith("0x") \
                    and not re.match(r"^[0-9a-f]+$", row[0].lower()):
                continue
            if len(row) > col:
                yield row[col]


def lines(path):
    if not os.path.exists(path):
        return
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            yield line.strip()


def identifiers_in(root, exts=("gsc", "csc", "gsh")):
    """Every identifier-looking word of the scripts under root (names of calls, fields...)."""
    word = re.compile(rb"[A-Za-z_][A-Za-z0-9_]*")
    seen = set()
    for dp, dn, fn in os.walk(root):
        for f in fn:
            if f.rsplit(".", 1)[-1].lower() not in exts:
                continue
            with open(os.path.join(dp, f), "rb") as fh:
                for m in word.finditer(fh.read()):
                    w = m.group().lower()
                    if w not in seen:
                        seen.add(w)
    return sorted(x.decode("latin1") for x in seen)


IDENT_B = re.compile(rb"[A-Za-z_][A-Za-z0-9_]{1,63}")


def words_of_file(path, maxsize=700 << 20):
    """The identifier-looking words (2 to 64 characters, lower-cased) of any file; a missing,
    unreadable or larger than `maxsize` file has none."""
    try:
        if os.path.getsize(path) > maxsize:
            return set()
        with open(path, "rb") as f:
            data = f.read()
    except OSError:
        return set()
    return {m.group().lower().decode("latin1") for m in IDENT_B.finditer(data)}


def words_of(paths):
    """The words (`words_of_file`) of every file of `paths`."""
    out = set()
    for p in paths:
        out |= words_of_file(p)
    return out


def files_under(root, exts=None):
    """Every file under root (with one of `exts`, lower-case, when given), skipping VCS and build
    directories."""
    for dp, dn, fn in os.walk(root):
        dn[:] = [d for d in dn if d not in (".git", "__pycache__", "target", "node_modules")]
        for f in fn:
            if exts is None or f.rsplit(".", 1)[-1].lower() in exts:
                yield os.path.join(dp, f)
