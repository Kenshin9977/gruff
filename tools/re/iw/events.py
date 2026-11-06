"""Engine-emitted events of the IW games: every call site of the VM's notify, its event (an SL
string id: the engine's own strings are the scr_const slots its init fills), argument count and
types, measured under Unicorn. The game is never run.

    py -3.11 -I iw/events.py extract --game iw5   [--exe PATH]   -> out/events_iw5.json
    py -3.11 -I iw/events.py build --game iw5                     -> gruff-data/events/iw5.toml,
                                                                    reports/events_iw5.md

The method is in common/events.py, the driver in common/events_run.py; this file holds each
engine's anchors (each a hypothesis checked at run time).
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common import events_run as R  # noqa: E402

# var_typename of iw3mp.exe and iw4mp.exe (read by common.events.type_name_table), for the builds
# that carry no table; the two agree on the types a notify pushes (0-6)
IW_TYPES = ["undefined", "object", "string", "localized string", "vector", "float", "int"]

PROFILES = {
    "iw5": {
        "title": "Modern Warfare 3 (2011)", "exe_key": "exe.iw5",
        "extractor": "tools/re/iw/events.py",
        # Scr_NotifyNum_Internal(object id, SL string id, paramcount): Scr_Notify(ent, str, n)
        # (109 callers, found from the reads of the `trigger`/`movedone` scr_const slots)
        # tail-calls Scr_NotifyNum(entnum, classnum, str, n), which tail-calls this.
        "core": 0x2A18F0, "core_args": {"inst": None, "obj": 0, "event": 1, "count": 2},
        "hash": None, "require_cstr": False,
        # scr_const: one init, SL_GetString(char*, ...) per string (341 calls), 16-bit ids
        "const_init": [0x1899E0], "const_alloc": 0x17C900, "const_width": 2,
        # scrVmPub.top (rip-relative, read off the core: `mov rax, [top]; shl count, 4`):
        # 16-byte values, type tag at +8
        "vmpub_top": 0x31CF9C0, "vmpub_stride": 0, "value_size": 0x10, "type_off": 8,
        "top_disp": 0, "top_abs": True, "insts": (0,),
        # Scr_GetEntityId: the id Scr_AddEntity (0x1a6b90) pushes (the wrappers resolve with
        # FindEntityId 0x29b580)
        "entity_sources": [0x29EDE0],
        "typenames": IW_TYPES, "typenames_from": "iw4mp.exe and iw3mp.exe var_typename, equal on 0-6",
        "corpus": ["iw5/decompiled"],
        "vm_of_inst": {None: "gsc"},
    },
    "iw4": {
        "title": "Modern Warfare 2 (2009)", "exe_key": "exe.iw4",
        "extractor": "tools/re/iw/events.py",
        # IW5's chain: Scr_Notify (0x1a9530, 107 callers, from the `damage`/`death` slot reads)
        # -> Scr_NotifyNum (0x21d160) -> this (object id, SL string id, paramcount)
        "core": 0x21D060, "core_args": {"inst": None, "obj": 0, "event": 1, "count": 2},
        "hash": None, "require_cstr": False,
        "const_init": [0x18D470], "const_alloc": 0x1817D0, "const_width": 2,
        # scrVmPub.top, read off the core (`mov rax, [top]; sub rax, 0x10`)
        "vmpub_top": 0x22AFFB0, "vmpub_stride": 0, "value_size": 0x10, "type_off": 8,
        # Scr_GetEntityId: the id Scr_AddEntity (0x1a8e40) pushes
        "entity_sources": [0x21AA10],
        "top_disp": 0, "top_abs": True, "insts": (0,),
        "corpus": ["iw4/source"],
        "vm_of_inst": {None: "gsc"},
    },
    "iw3": {
        "title": "Call of Duty 4", "exe_key": "exe.iw3", "extractor": "tools/re/iw/events.py",
        # x86. Scr_NotifyNum(entnum, classnum, SL string id), paramcount in eax (usercall):
        # 71 callers, found from the `trigger`/`damage` slot reads; it resolves the entity and
        # calls VM_Notify (0x51c760) itself.
        "core": 0x11CDE0, "core_args": {"inst": None, "obj": 0, "event": 2, "count": ("reg", "eax")},
        "core_recv": "entity",
        "hash": None, "require_cstr": False,
        "const_init": [0xE6340], "const_alloc": 0x112DA0, "const_width": 2,
        # scrVmPub.top at 0x178c068 (absolute): 8-byte values, type tag at +4
        "vmpub_top": 0x138C068, "vmpub_stride": 0, "value_size": 8, "type_off": 4,
        "top_disp": 0, "top_abs": True, "top_width": "dword", "insts": (0,),
        # Scr_GetEntityId: its result (eax) is the id Scr_AddObject pushes right after
        "entity_sources": [0x116A80],
        "corpus": ["iw3/source"],
        "vm_of_inst": {None: "gsc"},
    },
    "iw6": {
        "title": "Ghosts", "exe_key": "exe.iw6", "extractor": "tools/re/iw/events.py",
        # IW5's chain with 32-bit string ids: Scr_Notify (0x38ffb0, 199 callers, from the
        # `damage`/`death` slot reads) -> Scr_NotifyNum (0x41f4c0) -> this (id, string, count)
        "core": 0x41F3E0, "core_args": {"inst": None, "obj": 0, "event": 1, "count": 2},
        "hash": None, "require_cstr": False,
        "const_init": [0x3A7F40], "const_alloc": 0x37D650, "const_width": 4,
        "entity_sources": [0x41C700],  # Scr_GetEntityId (the id Scr_AddEntity pushes)
        # scrVmPub.top read off the core
        "vmpub_top": 0x575C630, "vmpub_stride": 0, "value_size": 0x10, "type_off": 8,
        "top_disp": 0, "top_abs": True, "insts": (0,),
        "typenames": IW_TYPES, "typenames_from": "iw4mp.exe and iw3mp.exe var_typename, equal on 0-6",
        "corpus": ["iw6/decompiled"],
        "vm_of_inst": {None: "gsc"},
    },
    "h1": {
        "title": "Modern Warfare Remastered", "exe_key": "exe.h1",
        "extractor": "tools/re/iw/events.py",
        # Scr_Notify (0x4599c0, 197 callers) -> Scr_NotifyNum (0x510420) -> this
        "core": 0x510340, "core_args": {"inst": None, "obj": 0, "event": 1, "count": 2},
        "hash": None, "require_cstr": False,
        "const_init": [0x468F40], "const_alloc": 0x508880, "const_width": 4,
        "entity_sources": [0x50D830],  # Scr_GetEntityId (the id Scr_AddEntity pushes)
        "vmpub_top": 0xB7AE3E0, "vmpub_stride": 0, "value_size": 0x10, "type_off": 8,
        "top_disp": 0, "top_abs": True, "insts": (0,),
        "typenames": IW_TYPES, "typenames_from": "iw4mp.exe and iw3mp.exe var_typename, equal on 0-6",
        "corpus": ["h1/decompiled"],
        "vm_of_inst": {None: "gsc"},
    },
    "s1": {
        "title": "Advanced Warfare", "exe_key": "exe.s1_sp", "extractor": "tools/re/iw/events.py",
        # the campaign dump: the multiplayer dump (exe.s1) holds almost none of its strings in
        # clear (no `movedone`, no `tag_origin`), so its scr_const init cannot be read.
        # Scr_Notify (0x26b2b0, 211 callers) -> Scr_NotifyNum (0x31cc60) -> this
        "core": 0x31CB80, "core_args": {"inst": None, "obj": 0, "event": 1, "count": 2},
        "hash": None, "require_cstr": False,
        "const_init": [0x283970], "const_alloc": 0x2556E0, "const_width": 4,
        "entity_sources": [0x31A020],  # Scr_GetEntityId (the id Scr_AddEntity pushes)
        "vmpub_top": 0xA1938E0, "vmpub_stride": 0, "value_size": 0x10, "type_off": 8,
        "top_disp": 0, "top_abs": True, "insts": (0,),
        "typenames": IW_TYPES, "typenames_from": "iw4mp.exe and iw3mp.exe var_typename, equal on 0-6",
        "corpus": ["s1/decompiled"],
        "vm_of_inst": {None: "gsc"},
    },
    "iw7": {
        "title": "Infinite Warfare", "exe_key": "exe.iw7", "extractor": "tools/re/iw/events.py",
        # Scr_Notify (0xb51b80, 352 callers) -> Scr_NotifyNum (0xc0c390) -> this
        "core": 0xC0C2B0, "core_args": {"inst": None, "obj": 0, "event": 1, "count": 2},
        "hash": None, "require_cstr": False,
        "const_init": [0xB727B0], "const_alloc": 0xC03C70, "const_width": 4,
        "entity_sources": [0xC08FA0],  # Scr_GetEntityId (the id Scr_AddEntity pushes)
        "vmpub_top": 0x6B183D0, "vmpub_stride": 0, "value_size": 0x10, "type_off": 8,
        "top_disp": 0, "top_abs": True, "insts": (0,),
        "typenames": IW_TYPES, "typenames_from": "iw4mp.exe and iw3mp.exe var_typename, equal on 0-6",
        "corpus": ["iw7/decompiled"],
        "vm_of_inst": {None: "gsc"},
    },
}

if __name__ == "__main__":
    R.main(PROFILES)
