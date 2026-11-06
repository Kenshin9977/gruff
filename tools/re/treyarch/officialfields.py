"""Reader of docs_modtools/ScriptObjectFields.xlsx (BO3 mod tools), read locally and used only as an
oracle for the field extraction: per sheet (object kind), the field names and the columns the
sheet gives. No text of the sheet is copied into any output; only names are compared and counted.
Standard library only (zipfile + ElementTree): no openpyxl needed.

    py -3.11 -I officialfields.py [path]      # prints per-sheet counts and the header row
"""
import re
import zipfile
import xml.etree.ElementTree as ET
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common import paths  # noqa: E402

# the mod tools install: bo3_root in tools/re/paths.local.json, or GSC_RE_BO3_ROOT
BO3 = paths.get("bo3_root") or r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III"

DEFAULT = os.path.join(BO3, "docs_modtools", "ScriptObjectFields.xlsx")
NS = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
      "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships"}
RELNS = "{http://schemas.openxmlformats.org/package/2006/relationships}"


def _col(ref):
    m = re.match(r"([A-Z]+)", ref)
    n = 0
    for c in m.group(1):
        n = n * 26 + ord(c) - 64
    return n - 1


def read_sheets(path=DEFAULT):
    """{sheet name: [row tuples]} with shared strings resolved."""
    z = zipfile.ZipFile(path)
    sst = []
    if "xl/sharedStrings.xml" in z.namelist():
        root = ET.fromstring(z.read("xl/sharedStrings.xml"))
        for si in root.findall("m:si", NS):
            sst.append("".join(t.text or "" for t in si.iter("{%s}t" % NS["m"])))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    target = {r.get("Id"): r.get("Target") for r in rels.iter(RELNS + "Relationship")}
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    out = {}
    for sh in wb.find("m:sheets", NS):
        rid = sh.get("{%s}id" % NS["r"])
        p = "xl/" + target[rid].lstrip("/").replace("xl/", "")
        root = ET.fromstring(z.read(p))
        rows = []
        for row in root.iter("{%s}row" % NS["m"]):
            cells = {}
            for c in row.findall("m:c", NS):
                v = c.find("m:v", NS)
                t = c.get("t")
                if t == "inlineStr":
                    val = "".join(x.text or "" for x in c.iter("{%s}t" % NS["m"]))
                elif v is None:
                    continue
                elif t == "s":
                    val = sst[int(v.text)]
                else:
                    val = v.text
                cells[_col(c.get("r"))] = val
            if cells:
                rows.append(tuple(cells.get(i) for i in range(max(cells) + 1)))
        out[sh.get("name")] = rows
    return out


IDENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def field_names(path=DEFAULT):
    """{sheet: {lower-cased name: type cell or None}}. Each sheet's first row is its title; every
    later row whose first cell is an identifier is one field (the second cell is its F_ type, or
    '0' where the field has its own getter/setter). Preprocessor and comment rows are skipped.
    The sheet has no read-only column."""
    res = {}
    for sheet, rows in read_sheets(path).items():
        fields = {}
        for r in rows[1:]:
            n = (r[0] or "").strip()
            if not IDENT.match(n):
                continue
            ty = (r[1] or "").strip() if len(r) > 1 and r[1] is not None else None
            fields[n.lower()] = ty
        res[sheet] = fields
    return res


if __name__ == "__main__":
    p = sys.argv[1] if len(sys.argv) > 1 else DEFAULT
    sheets = read_sheets(p)
    for s, rows in sheets.items():
        print(s, len(rows), rows[0] if rows else None, rows[1] if len(rows) > 1 else None)
    for s, f in field_names(p).items():
        print(s, len(f))
