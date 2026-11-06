"""This machine's locations for the RE tools: executables, dumps, name lists and corpora.

None of them is written in the repository. A key is looked up, in order:

1. the environment variable `GSC_RE_<KEY>` (upper case, dots as underscores: `exe.t7` ->
   `GSC_RE_EXE_T7`; a list key takes several paths separated by `os.pathsep`);
2. `tools/re/paths.local.json` (gitignored; copy `tools/re/paths.example.json`, which lists every
   key, and set your own paths; dotted keys are nested objects: `{"exe": {"t7": "..."}}`);
3. the default below, for the few locations that sit next to the repositories (the corpus and
   gruff-data beside gruff in one toolchain folder; for `gsc_data`, `$GRUFF_DATA` or `$GSC_DATA`
   first, then a `gruff-data` folder, else the older `gsc-data`).

`need(key)` stops with a message naming the variable and the file when the key is set nowhere;
`get(key)` returns None instead. Command-line arguments (`--exe`...) still win over both.
"""
import json
import os

RE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # tools/re
LOCAL = os.path.join(RE, "paths.local.json")
EXAMPLE = os.path.join(RE, "paths.example.json")
TOOLCHAIN = os.path.abspath(os.path.join(RE, "..", "..", ".."))  # the repository's parent folder


def _data_default():
    """`$GRUFF_DATA` or `$GSC_DATA`, else `gruff-data` (or the older `gsc-data`) beside the repo."""
    for var in ("GRUFF_DATA", "GSC_DATA"):
        if os.environ.get(var):
            return os.environ[var]
    for name in ("gruff-data", "gsc-data"):
        d = os.path.join(TOOLCHAIN, name)
        if os.path.isdir(d):
            return d
    return os.path.join(TOOLCHAIN, "gruff-data")


DEFAULTS = {
    "corpus": os.path.join(TOOLCHAIN, "corpus"),
    "gsc_data": _data_default(),
    "hash_packages": os.path.join(TOOLCHAIN, "hash-packages"),
}
_cache = None


def _local():
    global _cache
    if _cache is None:
        _cache = {}
        if os.path.isfile(LOCAL):
            with open(LOCAL, encoding="utf-8") as f:
                _cache = json.load(f)
    return _cache


def env_name(key):
    return "GSC_RE_" + key.upper().replace(".", "_").replace("-", "_")


def get(key, default=None):
    """The path for `key` (a string, or a list for a list key), or `default`."""
    v = os.environ.get(env_name(key))
    if v:
        return v
    node = _local()
    for part in key.split("."):
        node = node.get(part) if isinstance(node, dict) else None
    if node not in (None, "", []):
        return node
    return DEFAULTS.get(key, default)


def get_list(key):
    """A list key (several roots): [] when unset."""
    v = os.environ.get(env_name(key))
    if v:
        return [p for p in v.split(os.pathsep) if p]
    node = _local()
    for part in key.split("."):
        node = node.get(part) if isinstance(node, dict) else None
    v = node or []
    return [v] if isinstance(v, str) else list(v)


def need(key):
    v = get(key)
    if not v:
        raise SystemExit("%s: no path for `%s`: set %s or add it to %s (template: %s)" % (
            os.path.basename(LOCAL), key, env_name(key), LOCAL, EXAMPLE))
    return v
