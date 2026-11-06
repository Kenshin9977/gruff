"""Call sites of a script corpus: argument counts of every call, by name, syntax kind (function
or method) and file type. Reads .gsc/.csc/.gsh in memory and stores no content: only (name, kind,
VM, argument count) counts.

A small tokenizer, not a parser: comments and strings are blanked, `name(` is a call, it is a
method call when the token before it (after `thread`/`childthread`) ends an expression (an
identifier that is not a keyword, `]` or `)`), and the arguments are the top-level commas.
Namespaced calls (`ns::f(`), function definitions, `&f` references and `[[ ]]` pointer calls are
skipped, and so is any name the scripts define themselves.

Decompiled corpora name unknown symbols by their hash (`function_d279413d`, `function_<16 hex>`);
`hashed_name` turns such a name into its hash, so a call site can be matched to a table entry by
hash when no string is known.
"""
import collections
import os
import re

# language keywords; `intrinsics` (per engine) are added by the caller
KEYWORDS = {
    "return", "if", "else", "while", "for", "foreach", "in", "switch", "case", "default", "wait",
    "waittill", "waittillmatch", "waittillframeend", "notify", "endon", "thread", "childthread",
    "function", "private", "autoexec", "new", "class", "var", "const", "do", "break", "continue",
    "true", "false", "undefined", "self", "level", "game", "anim", "world",
}
OBJECT_WORDS = {"self", "level", "game", "anim", "world"}
TOKEN = re.compile(r"[A-Za-z_][\w]*|::|[()\[\],;{}]|\S")
CONTROL = {"if", "while", "for", "foreach", "switch"}
HASHED = re.compile(r"^function_([0-9a-f]{1,16})$")


def count_args(toks, i):
    """The argument count of the call whose `(` is toks[i]: its top-level commas + 1, 0 when
    nothing sits between the parentheses."""
    depth, j, nargs, empty = 0, i, 0, True
    while j < len(toks):
        tk = toks[j]
        if tk in ("(", "[", "{"):
            depth += 1
        elif tk in (")", "]", "}"):
            depth -= 1
            if depth == 0:
                break
        elif tk == "," and depth == 1:
            nargs += 1
        elif depth >= 1 and j > i:
            empty = False
        j += 1
    return 0 if empty else nargs + 1


def hashed_name(name):
    m = HASHED.match(name)
    return int(m.group(1), 16) if m else None


def blank(src, doc_comments=True, dev_markers=False):
    """Blank comments and string literals, keeping offsets. `doc_comments`: `/@ ... @/` blocks
    are comments too (Treyarch); `dev_markers`: `/#` and `#/` are blanked and the code between
    them kept (the IW scans)."""
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif src.startswith("/*", i):
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append(re.sub(r"[^\n]", " ", src[i:j]))
            i = j
        elif doc_comments and src.startswith("/@", i):  # doc comment blocks /@ ... @/
            j = src.find("@/", i + 2)
            j = n if j < 0 else j + 2
            out.append(re.sub(r"[^\n]", " ", src[i:j]))
            i = j
        elif dev_markers and (src.startswith("/#", i) or src.startswith("#/", i)):
            out.append("  ")  # dev block markers: the code inside is kept
            i += 2
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            out.append('""' + " " * max(0, j - i - 1))
            i = j + 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def scan(root, intrinsics=(), exts=("gsc", "csc", "gsh"), vm_of=None):
    """(calls, defined, files): calls[(name_lower, kind, vm, nargs)] = sites; defined = names the
    scripts define. `vm_of(path, ext)` -> "gsc" | "csc" | "gsh" (default: by extension)."""
    kw = KEYWORDS | {i.lower() for i in intrinsics}
    calls = collections.Counter()
    defined = set()
    files = 0
    for dp, dn, fn in os.walk(root):
        for f in fn:
            ext = f.rsplit(".", 1)[-1].lower()
            if ext not in exts:
                continue
            files += 1
            p = os.path.join(dp, f)
            with open(p, "rb") as fh:
                src = blank(fh.read().decode("latin1"))
            vm = vm_of(p, ext) if vm_of else ("csc" if ext == "csc" else "gsc" if ext == "gsc"
                                              else "gsh")
            scan_source(src, vm, kw, calls, defined)
    return calls, defined, files


def scan_source(src, vm, kw, calls, defined):
    toks = [m.group() for m in TOKEN.finditer(src)]
    opener = {}
    stack = []
    for i, t in enumerate(toks):
        if t == "(":
            stack.append(i)
        elif t == ")" and stack:
            opener[i] = stack.pop()
    for i, t in enumerate(toks):
        if t != "(" or i == 0:
            continue
        name = toks[i - 1]
        if not re.match(r"[A-Za-z_]", name):
            continue
        prev = toks[i - 2] if i >= 2 else ""
        if prev in ("function", "define") or (prev in ("private", "autoexec") and i >= 3):
            defined.add(name.lower())
            continue
        if prev == "::" or prev == "&" or name.lower() in kw - OBJECT_WORDS:
            continue
        if prev == ".":
            continue
        nargs = count_args(toks, i)
        k = i - 2
        while k >= 0 and toks[k] in ("thread", "childthread"):
            k -= 1
        owner = toks[k] if k >= 0 else ""
        if owner == ")" and opener.get(k, 0) > 0 and toks[opener[k] - 1].lower() in CONTROL:
            owner = "if"  # `if ( x ) f()`: that parenthesis closes a condition
        if k >= 1 and toks[k - 1] == "define":
            owner = "#"  # `#define NAME f()`: NAME is the macro, not an object
        is_method = owner in (")", "]") or (
            re.match(r"[A-Za-z_]\w*$", owner) is not None
            and (owner.lower() not in kw or owner.lower() in OBJECT_WORDS))
        calls[(name.lower(), "method" if is_method else "function", vm, nargs)] += 1
