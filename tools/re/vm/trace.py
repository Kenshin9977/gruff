"""gsc-trace: the structured trace format of a GSC script VM run, shared by every emitter.

Format spec: docs/trace-format.md (version FORMAT_VERSION). One JSON object per line (JSONL):
a `header` record, then events in the order the VM produced them, then an `end` record.
This module is game-agnostic: an emitter (tools/re/vm/t7_trace.py for Black Ops III; a T8/T9
emulator, a Rust tracer or a DAP adapter later) builds records with `Trace.emit` and maps code
addresses to source with `SourceMap`; viewers read them with `read`.

Keep this module stable: other emitters import it. Additive changes only (new optional keys,
new event kinds) within a major version; anything else bumps FORMAT_VERSION.
Stdlib only.
"""
import bisect
import json

FORMAT = "gsc-trace"
FORMAT_VERSION = 1

# event kinds of version 1 and the keys each carries besides seq, t, ev (see the spec)
EVENTS = {
    "header": "format version game producer time_unit frame_ms sources options",
    "frame": "",
    "thread_create": "th id parent kind entry self loc entry_loc",
    "thread_end": "th reason by obj event loc",
    "wait_start": "th kind duration obj event match loc",
    "wait_end": "th kind since wait by loc",
    "notify": "th obj event args loc",
    "endon": "th obj event loc",
    "builtin": "th name method self args loc",
    "error": "th msg terminal loc",
    "call": "th func loc from",
    "return": "th func loc",
    "op": "th op loc recovery",
    "end": "status error frames counts dropped",
}
WAIT_KINDS = ("wait", "waittill", "waittillmatch", "waittillframeend", "wait_other")
THREAD_END_REASONS = ("return", "endon", "error", "killed")


class Trace:
    """Collects records; `seq` numbers them in emission order. `caps` limits a kind (e.g. op):
    records past the cap are counted in end.dropped, not written."""

    def __init__(self, game, producer, time_unit="frame", frame_ms=None, options=None,
                 caps=None):
        self.records = []
        self.seq = 0
        self.caps = dict(caps or {})
        self.counts = {}
        self.dropped = {}
        self.sources = []
        self.header = {"ev": "header", "format": FORMAT, "version": FORMAT_VERSION,
                       "game": game, "producer": producer, "time_unit": time_unit,
                       "frame_ms": frame_ms, "sources": self.sources,
                       "options": dict(options or {})}

    def add_source(self, path, file=None, lines=False, **extra):
        """Register a script; returns its index (the `src` of a loc)."""
        d = {"id": len(self.sources), "path": path}
        if file:
            d["file"] = file
        d["lines"] = bool(lines)
        d.update(extra)
        self.sources.append(d)
        return d["id"]

    def emit(self, ev, t, /, **kw):  # positional-only: an event may carry a "self" key
        n = self.counts.get(ev, 0)
        cap = self.caps.get(ev)
        if cap is not None and n >= cap:
            self.dropped[ev] = self.dropped.get(ev, 0) + 1
            return None
        self.counts[ev] = n + 1
        rec = {"seq": self.seq, "t": t, "ev": ev}
        rec.update({k: v for k, v in kw.items() if v is not None})
        self.seq += 1
        self.records.append(rec)
        return rec

    def finish(self, t, status, error=None, frames=None):
        return self.emit("end", t, status=status, error=error, frames=frames,
                         counts=dict(self.counts), dropped=dict(self.dropped) or None)

    def lines(self):
        yield json.dumps(self.header, separators=(",", ":"))
        for r in self.records:
            yield json.dumps(r, separators=(",", ":"), ensure_ascii=False)

    def write(self, path_or_file):
        if hasattr(path_or_file, "write"):
            for ln in self.lines():
                path_or_file.write(ln + "\n")
            return
        with open(path_or_file, "w", encoding="utf-8", newline="\n") as f:
            self.write(f)


def read(path):
    """(header, [events]) of a trace file; refuses another format or a newer major version."""
    with open(path, encoding="utf-8") as f:
        recs = [json.loads(ln) for ln in f if ln.strip()]
    if not recs or recs[0].get("ev") != "header" or recs[0].get("format") != FORMAT:
        raise ValueError("%s: not a %s file" % (path, FORMAT))
    if recs[0].get("version", 0) > FORMAT_VERSION:
        raise ValueError("%s: %s version %s, this reader knows %d"
                         % (path, FORMAT, recs[0].get("version"), FORMAT_VERSION))
    return recs[0], recs[1:]


class SourceMap:
    """Code address -> loc {src, func, off[, line]}.

    Each loaded script: its base address and size in VM memory, its functions (start offset,
    qualified name) and, when the compiler gave one, a line table: `line_starts[i]` is the code
    offset where source line i+1 starts (lines without code repeat the next line's offset, as
    the Treyarch .gdb tables do), so the line of an offset is the last line starting at or
    before it."""

    def __init__(self):
        self.scripts = []  # (base, end, src, [starts], [names], line_starts or None)
        self._bases = []

    def add(self, base, size, src, functions, line_starts=None):
        fs = sorted(functions)
        self.scripts.append((base, base + size, src, [a for a, _ in fs], [n for _, n in fs],
                             list(line_starts) if line_starts else None))
        self.scripts.sort()
        self._bases = [s[0] for s in self.scripts]

    def script_of(self, addr):
        i = bisect.bisect_right(self._bases, addr) - 1
        if i >= 0 and addr < self.scripts[i][1]:
            return self.scripts[i]
        return None

    def function_at(self, addr):
        """Qualified name if addr is the first instruction of a function, else None."""
        s = self.script_of(addr)
        if not s:
            return None
        off = addr - s[0]
        i = bisect.bisect_left(s[3], off)
        return s[4][i] if i < len(s[3]) and s[3][i] == off else None

    def loc(self, addr):
        s = self.script_of(addr) if addr else None
        if not s:
            return {"addr": "%#x" % addr} if addr else None
        base, _, src, starts, names, lines = s
        off = addr - base
        d = {"src": src, "off": off}
        i = bisect.bisect_right(starts, off) - 1
        if i >= 0:
            d["func"] = names[i]
        if lines:
            ln = bisect.bisect_right(lines, off)
            if ln >= 1:
                d["line"] = ln
        return d


def loc_text(loc, sources=None):
    """`file:line func` (or `func+0xoff`) for display."""
    if not loc:
        return "?"
    if "src" not in loc:
        return loc.get("addr", "?")
    name = str(loc["src"])
    if sources and loc["src"] < len(sources):
        p = sources[loc["src"]]
        name = (p.get("file") or p["path"]).replace("\\", "/").rsplit("/", 1)[-1]
    func = loc.get("func", "?")
    if "line" in loc:
        return "%s:%d %s" % (name, loc["line"], func)
    return "%s %s+%#x" % (name, func, loc["off"])
