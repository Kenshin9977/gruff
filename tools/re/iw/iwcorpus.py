"""Second measure: argument counts of every call site in a game's corpus (corpus/<game>/), by
name and syntax kind. Reads the scripts in memory and keeps counts only (the corpus is local and
never committed). The tokenizer follows common/corpus.py (its `blank`, dev-block markers
blanked, and `count_args`): comments and strings blanked, `name(` is a call, a method call when the token
before it (after thread/childthread) ends an expression, arguments are the top-level commas.
Namespaced calls (`a\\b::f(`, `ns::f(`), definitions and pointer calls `[[ ]]` are skipped, and
so is every name the scripts define.

Decompiled names: gsc-tool writes an unnamed builtin `_func_<hex id>` / `_meth_<hex id>`; acts
writes a hashed one `function_<hex>`. Both are kept as written; the build step maps them.
"""
import collections
import os
import re
import sys

sys.path.insert(1, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))  # tools/re
from common import corpus as _common_corpus  # noqa: E402

KEYWORDS = {
    "return", "if", "else", "while", "for", "foreach", "in", "switch", "case", "default", "wait",
    "waittill", "waittillmatch", "waittillframeend", "notify", "endon", "thread", "childthread",
    "function", "private", "autoexec", "new", "class", "var", "const", "do", "break", "continue",
    "isdefined", "assert", "assertex", "assertmsg", "waitframe", "true", "false", "undefined",
    "self", "level", "game", "anim", "world", "breakpoint", "prof_begin", "prof_end",
    "isarray", "istrue", "vectorscale",
}
# keywords of the language, never builtins, in every IW dialect: calls to them are not counted
NOT_CALLS = KEYWORDS - {"self", "level", "game", "anim", "world"}
OBJECT_WORDS = {"self", "level", "game", "anim", "world"}
TOKEN = re.compile(r"[A-Za-z_][\w]*|::|[()\[\],;{}]|\S")
CONTROL = {"if", "while", "for", "foreach", "switch"}


def blank(src):
    return _common_corpus.blank(src, doc_comments=False, dev_markers=True)


def scan(root, exts=("gsc", "gsh", "csc")):
    calls = collections.Counter()   # (name_lower, kind, nargs) -> count
    files_of = collections.defaultdict(set)
    defined = set()
    files = 0
    for dp, dn, fn in os.walk(root):
        if os.path.basename(dp).startswith("_export"):
            continue
        for f in fn:
            ext = f.rsplit(".", 1)[-1].lower()
            if ext not in exts:
                continue
            files += 1
            with open(os.path.join(dp, f), "rb") as fh:
                src = blank(fh.read().decode("latin1"))
            toks = [m.group() for m in TOKEN.finditer(src)]
            opener = {}
            stack = []
            for i, t in enumerate(toks):
                if t == "(":
                    stack.append(i)
                elif t == ")" and stack:
                    opener[i] = stack.pop()
            depth_brace = 0
            for i, t in enumerate(toks):
                if t == "{":
                    depth_brace += 1
                elif t == "}":
                    depth_brace -= 1
                if t != "(" or i == 0:
                    continue
                name = toks[i - 1]
                if not re.match(r"[A-Za-z_]", name):
                    continue
                prev = toks[i - 2] if i >= 2 else ""
                low = name.lower()
                # a definition: at file scope (brace depth 0), `name(...) {`
                if depth_brace == 0 and prev not in ("=", "::"):
                    j = opener_close(toks, i)
                    if j is not None and j + 1 < len(toks) and toks[j + 1] == "{":
                        defined.add(low)
                        continue
                if prev in ("function", "define") or (prev in ("private", "autoexec") and i >= 3):
                    defined.add(low)
                    continue
                if prev == "::" or prev == "&" or low in NOT_CALLS or prev == ".":
                    continue
                nargs = _common_corpus.count_args(toks, i)
                k = i - 2
                while k >= 0 and toks[k] in ("thread", "childthread"):
                    k -= 1
                owner = toks[k] if k >= 0 else ""
                if owner == ")" and opener.get(k, 0) > 0 and toks[opener[k] - 1].lower() in CONTROL:
                    owner = "if"
                if owner == ")" and opener.get(k, 0) > 0 and toks[opener[k] - 1] == "]" :
                    owner = ")"
                if k >= 1 and toks[k - 1] == "define":
                    owner = "#"
                is_method = owner in (")", "]") or (
                    re.match(r"[A-Za-z_]\w*$", owner) is not None
                    and (owner.lower() not in KEYWORDS or owner.lower() in OBJECT_WORDS))
                # `[[ f ]]( )` pointer calls: name is `]`, skipped above
                kind = "method" if is_method else "function"
                calls[(low, kind, nargs)] += 1
                files_of[(low, kind)].add(f)
    return calls, defined, files


def opener_close(toks, i):
    d = 0
    for j in range(i, len(toks)):
        if toks[j] == "(":
            d += 1
        elif toks[j] == ")":
            d -= 1
            if d == 0:
                return j
    return None
