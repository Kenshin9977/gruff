"""Show a gsc-trace (docs/trace-format.md) as a per-frame timeline with one lane per thread.

    py -3.11 -I tools/re/vm/trace_view.py run.jsonl [--width 34] [--ops] [--calls] [--thread T1]
    py -3.11 -I tools/re/vm/trace_view.py run.jsonl --html timeline.html

Text: one row per event, under a `-- f<frame>` rule each time the VM time moves (frames where
nothing happens are folded). A lane holds `|` while its thread runs or is runnable, `:` while it
waits, and the event text in the row of its own events. Marks: `+` created, `x` ended, `~` wait
starts, `^` wait ends (woken), `!` notify, `#` endon, `E` script error, `>` call, `<` return,
`.` opcode; `=> T1` in the parent's lane where it starts a thread. `[n]` is the record seq, so
`by [9]` points at the notify that woke or killed a thread.

--html writes one self-contained page (no network): a swimlane chart (x: event order, frames
marked; y: threads; bars run/wait; notify -> wake/kill links) above the same table.
Stdlib only; reads any producer's trace.
"""
import argparse
import html
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from trace import read, loc_text  # noqa: E402

MARK = {"thread_create": "+", "thread_end": "x", "wait_start": "~", "wait_end": "^",
        "notify": "!", "endon": "#", "error": "E", "call": ">", "return": "<", "op": ".",
        "builtin": ""}


def line_of(loc):
    return ("L%d" % loc["line"]) if loc and "line" in loc else (
        ("+%#x" % loc["off"]) if loc and "off" in loc else "")


def short_func(f):
    return (f or "?").split("::")[-1]


def describe(e):
    """Short text of one event, for a lane cell."""
    k = e["ev"]
    ln = line_of(e.get("loc"))
    if k == "thread_create":
        s = "%s() %s" % (short_func(e.get("entry")), line_of(e.get("entry_loc")))
        if e.get("self"):
            s = "%s %s" % (e["self"], s)
        return s.strip()
    if k == "thread_end":
        r = e.get("reason", "?")
        if r == "endon":
            return "endon %s %s by [%s]" % (e.get("obj", "?"), e.get("event", "?"), e.get("by"))
        return "%s %s" % (r, ln)
    if k == "wait_start":
        kind = e["kind"]
        if kind in ("wait", "wait_other"):
            return "%s %s %s" % (kind, e.get("duration", ""), ln)
        if kind == "waittillframeend":
            return "waittillframeend %s" % ln
        m = (" " + ",".join(e["match"])) if e.get("match") else ""
        return "%s %s %s%s %s" % (kind, e.get("obj", "?"), e.get("event", "?"), m, ln)
    if k == "wait_end":
        n = e["t"] - e["since"]
        by = (" by [%s]" % e["by"]) if e.get("by") is not None else ""
        return "woke +%df%s %s" % (n, by, ln)
    if k == "notify":
        a = (" " + ",".join(e["args"])) if e.get("args") else ""
        return "%s notify %s%s %s" % (e.get("obj", "?"), e.get("event", "?"), a, ln)
    if k == "endon":
        return "endon %s %s %s" % (e.get("obj", "?"), e.get("event", "?"), ln)
    if k == "builtin":
        s = "%s(%s) %s" % (e["name"], ", ".join(e.get("args", [])), ln)
        return ("%s %s" % (e["self"], s)) if e.get("method") else s
    if k == "error":
        return "%s%s %s" % ("TERMINAL " if e.get("terminal") else "", e.get("msg", ""), ln)
    if k == "call":
        return "%s() %s" % (short_func(e.get("func")), ln)
    if k == "return":
        return "%s %s" % (short_func(e.get("func")), ln)
    if k == "op":
        return "%s %s%s" % (e["op"], ln, " (recovery)" if e.get("recovery") else "")
    return k


class Lanes:
    """Assigns each thread a column for its lifetime; states: run, wait."""

    def __init__(self):
        self.col = {}
        self.state = {}
        self.free = []
        self.n = 0
        self.order = []

    def open(self, th):
        if th in self.col:
            return
        if self.free:
            c = self.free.pop(0)
        else:
            c, self.n = self.n, self.n + 1
        self.col[th] = c
        self.state[th] = "run"
        self.order.append(th)

    def close(self, th):
        c = self.col.pop(th, None)
        self.state.pop(th, None)
        if c is not None:
            self.free.append(c)
            self.free.sort()


def filtered(events, a):
    keep = []
    for e in events:
        k = e["ev"]
        if k == "op" and not a.ops:
            continue
        if k in ("call", "return") and not a.calls:
            continue
        if a.thread and e.get("th") not in a.thread and k != "frame":
            continue
        keep.append(e)
    return keep


def text(header, events, a, out):
    w = a.width
    srcs = header.get("sources", [])
    out.write("gsc-trace v%s  game %s  %s\n" % (header.get("version"), header.get("game"),
                                               header.get("producer", "")))
    for s in srcs:
        out.write("  src %d: %s%s\n" % (s["id"], s.get("file") or s["path"],
                                        "" if s.get("lines") else "  (no line table)"))
    lanes = Lanes()
    shown_t = None
    rows = []
    for e in filtered(events, a):
        k = e["ev"]
        if k in ("header", "end", "frame"):
            continue
        t = e["t"]
        if t != shown_t:
            if shown_t is not None and t > shown_t + 1:
                rows.append(("fold", (shown_t + 1, t - 1)))
            rows.append(("rule", t))
            shown_t = t
        th = e.get("th")
        if k == "thread_create":
            lanes.open(th)
        elif th not in lanes.col and th:
            lanes.open(th)
        cells = {}
        for name, c in lanes.col.items():
            cells[c] = "|" if lanes.state[name] == "run" else ":"
        c = lanes.col.get(th)
        txt = "%s %s" % (MARK.get(k, "?"), describe(e)) if MARK.get(k) else describe(e)
        txt = "[%d] %s" % (e["seq"], txt)
        if c is not None:
            cells[c] = txt
        if k == "thread_create" and e.get("parent") in lanes.col:
            pc = lanes.col[e["parent"]]
            cells[pc] = "=> %s" % th
        rows.append(("ev", (t, dict(cells), lanes.n, k == "error")))
        if k == "wait_start":
            lanes.state[th] = "wait"
        elif k == "wait_end":
            lanes.state[th] = "run"
        elif k == "thread_end":
            lanes.close(th)
    ncols = max([r[1][2] for r in rows if r[0] == "ev"] or [1])
    # lane titles: the threads that used each column, in order
    titles = {}
    head = []
    seen = {}
    lanes2 = Lanes()
    for e in filtered(events, a):
        if e["ev"] == "thread_create" or (e.get("th") and e.get("th") not in lanes2.col):
            if e.get("th"):
                lanes2.open(e["th"])
                seen.setdefault(lanes2.col[e["th"]], []).append(e["th"])
        if e["ev"] == "thread_end":
            lanes2.close(e["th"])
    for c in range(ncols):
        titles[c] = "/".join(seen.get(c, ["?"]))
        head.append(titles[c][:w - 1].ljust(w))
    out.write("\n%6s  %s\n" % ("", "".join(head).rstrip()))
    for kind, val in rows:
        if kind == "rule":
            out.write("-- f%d %s\n" % (val, "-" * max(0, ncols * w - 6)))
        elif kind == "fold":
            lo, hi = val
            out.write("   (f%d%s: nothing)\n" % (lo, "" if lo == hi else "-f%d" % hi))
        else:
            t, cells, n, _ = val
            parts = []
            for c in range(ncols):
                s = cells.get(c, " ")
                if len(s) > 2 and c < ncols - 1:
                    s = s[:w - 1]
                parts.append(s.ljust(w) if c < ncols - 1 else s)
            out.write("%6s  %s\n" % ("", "".join(parts).rstrip()))
    end = next((e for e in events if e["ev"] == "end"), None)
    if end:
        out.write("== end: %s%s, %s frames\n" % (end.get("status"),
                                                (" (%s)" % end["error"]) if end.get("error")
                                                else "", end.get("frames")))
        if end.get("dropped"):
            out.write("   dropped (caps): %s\n" % json.dumps(end["dropped"]))


# --- HTML -------------------------------------------------------------------------------------
CSS = """
:root{--bg:#fbfbfa;--fg:#1d1d1b;--mut:#6b6b66;--line:#d9d8d3;--run:#3b6fb6;--wait:#b9c9e3;
--notify:#c26a12;--err:#c0392b;--end:#555;--card:#ffffff;--hl:#fff3c4}
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){--bg:#17181a;--fg:#e8e6e1;
--mut:#9a988f;--line:#34363a;--run:#7aa7e8;--wait:#34496b;--notify:#f0a050;--err:#ff6b5b;
--end:#aaa;--card:#1f2124;--hl:#4a4220}}
:root[data-theme="dark"]{--bg:#17181a;--fg:#e8e6e1;--mut:#9a988f;--line:#34363a;--run:#7aa7e8;
--wait:#34496b;--notify:#f0a050;--err:#ff6b5b;--end:#aaa;--card:#1f2124;--hl:#4a4220}
body{background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,sans-serif;margin:0;padding:16px}
h1{font-size:18px;margin:0 0 4px}.mut{color:var(--mut)}
.wrap{overflow-x:auto;border:1px solid var(--line);border-radius:6px;background:var(--card)}
svg text{fill:var(--fg);font:11px ui-monospace,monospace}
table{border-collapse:collapse;font:12px ui-monospace,monospace;width:100%}
td,th{padding:2px 8px;border-bottom:1px solid var(--line);vertical-align:top;white-space:nowrap}
th{position:sticky;top:0;background:var(--card);text-align:left}
tr.rule td{background:var(--bg);color:var(--mut);font-weight:600}
tr.err td{color:var(--err)} td.lane{color:var(--mut)} tr:target td,tr.hl td{background:var(--hl)}
.legend span{margin-right:14px} .k{display:inline-block;width:10px;height:10px;margin-right:4px;
vertical-align:middle}
"""

JS = """
document.querySelectorAll('[data-seq]').forEach(function(el){
  el.addEventListener('click',function(){var r=document.getElementById('e'+el.dataset.seq);
  if(!r)return;document.querySelectorAll('tr.hl').forEach(function(x){x.classList.remove('hl')});
  r.classList.add('hl');r.scrollIntoView({block:'center'});});});
"""


def build_html(header, events, a):
    evs = [e for e in filtered(events, a) if e["ev"] not in ("header", "end", "frame")]
    srcs = header.get("sources", [])
    # x: one step per event, in order; when the time moves, a gap of one step per elapsed frame
    # (at most 4) and a frame mark, so that waits across frames show as bars
    steps, frame_marks, x, last_t = {}, [], 0, None
    for e in evs:
        if e["t"] != last_t:
            if last_t is not None:
                x += min(e["t"] - last_t, 4)
            frame_marks.append((x, e["t"]))
            last_t = e["t"]
        steps[e["seq"]] = x
        x += 1
    nx = max(x, 1)
    threads = []
    for e in evs:
        th = e.get("th")
        if th and th not in threads:
            threads.append(th)
    lane_y = {th: i for i, th in enumerate(threads)}
    sx, lh, left, top = 16, 26, 56, 22
    width = left + nx * sx + 20
    height = top + max(len(threads), 1) * lh + 10
    svg = ['<svg width="%d" height="%d" role="img" aria-label="thread timeline">' % (width,
                                                                                     height)]
    shown = set()
    for px, t in frame_marks:
        if t in shown:
            continue
        shown.add(t)
        X = left + px * sx
        svg.append('<line x1="%d" y1="%d" x2="%d" y2="%d" stroke="var(--line)"/>'
                   % (X, top - 6, X, height - 4))
        svg.append('<text x="%d" y="%d">f%d</text>' % (X + 2, top - 9, t))
    for th, i in lane_y.items():
        svg.append('<text x="4" y="%d">%s</text>' % (top + i * lh + 15, html.escape(th)))
    # bars: run / wait segments per thread
    state, start = {}, {}
    seq_pos = {}

    def seg(th, x0, x1, kind):
        y = top + lane_y[th] * lh + 6
        col = "var(--run)" if kind == "run" else "var(--wait)"
        svg.append('<rect x="%d" y="%d" width="%d" height="12" fill="%s" rx="2"/>'
                   % (left + x0 * sx, y, max((x1 - x0) * sx, 3), col))

    for e in evs:
        th, px = e.get("th"), steps[e["seq"]]
        seq_pos[e["seq"]] = (th, px)
        if th and th not in state:
            state[th], start[th] = "run", px
        if e["ev"] == "wait_start":
            seg(th, start[th], px + 1, "run")
            state[th], start[th] = "wait", px + 1
        elif e["ev"] == "wait_end":
            seg(th, start[th], px, "wait")
            state[th], start[th] = "run", px
        elif e["ev"] == "thread_end":
            seg(th, start[th], px + 1, state[th])
            state[th] = None
    for th, s in state.items():
        if s:
            seg(th, start[th], nx, s)
    # links: notify -> woken waiter / killed thread
    for e in evs:
        by = e.get("by")
        if by is not None and by in seq_pos and e["seq"] in seq_pos:
            (t0, x0), (t1, x1) = seq_pos[by], seq_pos[e["seq"]]
            col = "var(--err)" if e["ev"] == "thread_end" else "var(--notify)"
            svg.append('<line x1="%d" y1="%d" x2="%d" y2="%d" stroke="%s" stroke-width="1.5" '
                       'stroke-dasharray="3 2"/>' % (left + x0 * sx + 5, top + lane_y[t0] * lh + 12,
                                                    left + x1 * sx + 5, top + lane_y[t1] * lh + 12,
                                                    col))
    # markers
    for e in evs:
        if not e.get("th"):
            continue
        px = left + steps[e["seq"]] * sx + 5
        y = top + lane_y[e["th"]] * lh + 12
        k = e["ev"]
        tip = html.escape("[%d] f%d %s %s" % (e["seq"], e["t"], k, describe(e)))
        if k == "notify":
            svg.append('<path d="M%d %d l5 -5 l5 5 l-5 5z" fill="var(--notify)" data-seq="%d">'
                       '<title>%s</title></path>' % (px - 5, y, e["seq"], tip))
        elif k == "error":
            svg.append('<circle cx="%d" cy="%d" r="5" fill="var(--err)" data-seq="%d"><title>%s'
                       '</title></circle>' % (px, y, e["seq"], tip))
        elif k == "thread_end":
            svg.append('<text x="%d" y="%d" data-seq="%d" style="fill:var(--end)">x<title>%s'
                       '</title></text>' % (px - 3, y + 4, e["seq"], tip))
        elif k in ("thread_create", "builtin", "endon", "wait_start", "wait_end", "call"):
            svg.append('<circle cx="%d" cy="%d" r="2.5" fill="var(--fg)" data-seq="%d"><title>'
                       '%s</title></circle>' % (px, y, e["seq"], tip))
    svg.append("</svg>")
    # table
    rows = ['<tr><th>seq</th><th>f</th>%s<th>where</th></tr>'
            % "".join("<th>%s</th>" % html.escape(t) for t in threads)]
    last_t = None
    for e in evs:
        if e["t"] != last_t:
            gap = ""
            if last_t is not None and e["t"] > last_t + 1:
                gap = " (after f%d%s: nothing)" % (last_t + 1, "" if e["t"] == last_t + 2
                                                   else "-f%d" % (e["t"] - 1))
            rows.append('<tr class="rule"><td colspan="%d">f%d%s</td></tr>'
                        % (len(threads) + 3, e["t"], gap))
            last_t = e["t"]
        cells = []
        for th in threads:
            if th == e.get("th"):
                m = MARK.get(e["ev"], "")
                cells.append("<td>%s</td>" % html.escape((m + " " if m else "") + describe(e)))
            elif e["ev"] == "thread_create" and th == e.get("parent"):
                cells.append('<td class="lane">=&gt; %s</td>' % html.escape(e["th"]))
            else:
                cells.append("<td></td>")
        rows.append('<tr id="e%d"%s><td>%d</td><td>%d</td>%s<td class="lane">%s</td></tr>'
                    % (e["seq"], ' class="err"' if e["ev"] == "error" else "", e["seq"], e["t"],
                       "".join(cells),
                       html.escape(loc_text(e.get("loc") or e.get("entry_loc"), srcs))))
    end = next((e for e in events if e["ev"] == "end"), {})
    title = "GSC trace: " + ", ".join(os.path.basename(s.get("file") or s["path"]) for s in srcs)
    legend = ('<div class="legend mut"><span><i class="k" style="background:var(--run)"></i>'
              'running</span><span><i class="k" style="background:var(--wait)"></i>waiting</span>'
              '<span><i class="k" style="background:var(--notify)"></i>notify / wake link</span>'
              '<span><i class="k" style="background:var(--err)"></i>error / endon kill</span>'
              '<span>click a marker to find its row</span></div>')
    return ("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
            "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>GSC Trace Timeline</title>"
            "<style>%s</style></head><body><h1>%s</h1><p class=\"mut\">gsc-trace v%s, game %s, "
            "%s. End: %s%s, %s frames.</p>%s<div class=\"wrap\">%s</div><h2 style=\"font-size:15px"
            "\">Events</h2><div class=\"wrap\"><table>%s</table></div><script>%s</script></body>"
            "</html>\n" % (CSS, html.escape(title), header.get("version"),
                           html.escape(str(header.get("game"))),
                           html.escape(header.get("producer", "")),
                           html.escape(str(end.get("status"))),
                           html.escape(" (%s)" % end["error"]) if end.get("error") else "",
                           end.get("frames"), legend, "".join(svg), "".join(rows), JS))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("trace")
    ap.add_argument("--width", type=int, default=34, help="lane width (text)")
    ap.add_argument("--ops", action="store_true", help="show op records")
    ap.add_argument("--calls", action="store_true", help="show call/return records")
    ap.add_argument("--thread", action="append", help="only these threads (repeatable)")
    ap.add_argument("--html", metavar="FILE", help="write a self-contained HTML timeline")
    a = ap.parse_args()
    header, events = read(a.trace)
    if a.html:
        with open(a.html, "w", encoding="utf-8", newline="\n") as f:
            f.write(build_html(header, events, a))
        print(a.html)
        return
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass
    text(header, events, a, sys.stdout)


if __name__ == "__main__":
    main()
