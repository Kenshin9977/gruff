"""Engine-emitted events of the Treyarch games: every call site of the VM's notify, its event,
argument count and types, measured under Unicorn on the flat dump. The game is never run.

    py -3.11 -I treyarch/events.py extract --game t7   [--exe PATH]   -> out/events_t7.json
    py -3.11 -I treyarch/events.py build --game t7                     -> gruff-data/events/t7.toml,
                                                                         reports/events_t7.md

The method is in common/events.py, the driver in common/events_run.py; this file holds each
engine's anchors (each a hypothesis checked at run time: the check names what fails).
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from common import hashes  # noqa: E402
from common import events_run as R  # noqa: E402

M64 = (1 << 64) - 1

# BO6/BO7 value tags (byte +8 of a stack value): the builds keep no type names, so the tags
# are named from the push cores' code, the same in both; the others stay "tag<n>".
#   0 undefined: BO7 0xa4e3c80, BO6 0x81e1150 push the tag alone
#   1 pointer:   BO7 0xa4e3a70 pushes a 32-bit object id, then its AddRef
#   2 string:    BO7 0xa4e3c10, BO6 0x81e10d0: a C string turned into a 32-bit string id by
#                the scr_const getter (0xa4b88b0, 0x81c1480)
#   3 (2 floats: BO6 0x81e1260, 0x81c86f0 copies 2 dwords), kept as tag3
#   4 vector:    BO7 0xa4e3dd0 (0xa4c64a0), BO6 0x81e12e0 (0x81c8710) copy 3 dwords
#   6 float:     BO7 0xa4e3620, BO6 0x81e0be0: vmovss of xmm1
#   7 int:       BO7 0xa4e3760 (movsxd of edx), BO6 0x81e09a0 (movzx of dl), 64-bit stores
T10_TAGS = ["undefined", "pointer", "string", "tag3", "vector", "tag5", "float", "int"] + \
    ["tag%d" % i for i in range(8, 32)]

T11_NOTE = (
    "The engine passes the event as the full 64-bit FNV-1a of the name (the `iw9.xhash` "
    "algorithm, the spelling of the decompiled corpus's `#\"hash_<16 hex>\"`): 443 `movabs` of "
    "214 distinct corpus event hashes in the code, 162 of them right before calls to "
    "Scr_Notify(ent, event, count) 0x7487500 -> Scr_NotifyNum(ctx, entnum, classnum, event, "
    "count) 0xa4e16c0 -> the core 0xa4e15c0 (ctx, object id, event, count), which pops the "
    "pushed values off [[ctx + 0x688]] and calls VM_Notify 0xa4e0e50. Scr_NotifyLevel 0xa4e16b0 "
    "(the level id at ctx + 0x1c) tail-calls it too. The context is VM + 0x10 (the builtins' "
    "Scr_GetNumParam context), read by the wrappers as [0xdb17d50] + 0x10 (server VM 0xdb1f5e8) "
    "or [0xdb17d58] + 0x10 (client VM 0xdb6a460). Script-string ids: three scr_const inits call "
    "the getter 0xa4b88b0 (&out, char*, 0, 1) 2351 + 346 + 45 times (the strings in clear); "
    "the SL wrappers (0x7487530...) turn an id back into its text and hash it. Value tags: "
    "T10_TAGS in treyarch/events.py.")

T10_NOTE = (
    "No event hash in the code: events are 32-bit script-string ids. scr_const: three inits "
    "call the getter 0x81c1480 (&out, char*, 0x20, 1) 2501 + 382 + 59 times with the strings "
    "in clear (an encrypted-string flag in the first byte is never set on them), each id then "
    "stored in a global. Of the 87 code references to the slots of `death`, `damage`, "
    "`trigger`, `movedone`, `weapon_fired`, `goal`, `reload`, 29 load the id right before "
    "Scr_Notify(ent, event, count) 0x59a3400 (395 callers) -> Scr_NotifyNum 0x81de400 -> "
    "VM_NotifyVm 0x81e2cd0 (vm, object id, event, count), BO7's core (pops the pushed values "
    "off vm + 0xc4a8, tags the frame 0x10, calls VM_Notify 0x81e4400); Scr_NotifyLevel "
    "0x81de3e0 reaches it too. A second path does not go through it: the only other function "
    "that reads the in-param count (ctx + 0xc480) and tags the frame 0x10 is 0x81d7810, whose "
    "one caller VM_NotifyCtx 0x81d7660 (ctx = vm + 0x30, event, object id, count, callback) "
    "is called by 0x81d7630 (level), 0x81d75c0 (entity) and 0x81d77d0 (with a callback); both "
    "are cores of this extraction. Value tags: T10_TAGS in treyarch/events.py. The VM is "
    "0xb27d730 (the server VM of the builtin sets); no client VM is located in this dump. "
    "VM attribution: VM_NotifyVm reads its VM from the context inside Scr_NotifyNum, which "
    "the wrapper probes see as 0, so the sites of that path carry no `vm`. The campaign dump "
    "(merged) the same way: getter 0x4c53cd0 (inits 0x5f899b0, 0x5f9d530, 0x5fa0920: 2249 + "
    "375 + 59 calls, strings in clear), Scr_Notify 0x6be2910 -> Scr_NotifyNum 0x4c6f2a0 -> "
    "VM_NotifyVm 0x4c73c40 (VM 0xabd3a00, stack top at VM + 0x8388), VM_NotifyCtx 0x4c67600 "
    "(the caller of 0x4c67780); its value tags differ from multiplayer's (T10SP_TAGS).")

# the BO6 campaign tags differ from multiplayer's (read from its push cores): 3 vector
# (0x4c71bc0 and 0x4c71cc0 copy 2 or 3 floats), 4 four floats (0x4c5e540), 5 float (0x4c71350,
# xmm), 6 int (0x4c71130, 0x4c71490), 2 string (0x4c71a70 through the getter 0x4c53cd0)
T10SP_TAGS = ["undefined", "pointer", "string", "vector", "tag4", "float", "int"] + [
    "tag%d" % i for i in range(7, 32)]

# the BO6 campaign dump, merged into the t10 catalogue (`build --game t10`); found as in
# multiplayer (reports/events_t10.md): Scr_Notify 0x6be2910 -> Scr_NotifyNum 0x4c6f2a0 ->
# VM_NotifyVm 0x4c73c40; VM_NotifyCtx 0x4c67600 (the caller of 0x4c67780)
T10SP = {
    "title": "Black Ops 6 (campaign)", "exe_key": "exe.t10sp",
    "extractor": "tools/re/treyarch/events.py", "merge_into": "t10",
    "core": 0x4C73C40, "core_args": {"inst": 0, "obj": 1, "event": 2, "count": 3},
    "extra_cores": [(0x4C67600, {"inst": 0, "obj": 2, "event": 1, "count": 3})],
    "hash": None, "require_cstr": False,
    "const_init": [0x5F899B0, 0x5F9D530, 0x5FA0920], "const_alloc": 0x4C53CD0,
    "const_width": 4, "const_str_arg": 1, "const_out_arg": 0,
    "vm_of_inst": {}, "vm_of_ctx": {0xABD3A00: "gsc", 0xABD3A10: "gsc"},
    "vmpub_top": 0xABD3A10 + 0x8378, "vmpub_stride": 0, "insts": (0,),
    "top_disp": 0x8378, "top_store": True, "value_size": 0x10, "type_off": 8,
    "tag_width": "byte",
    "typenames": T10SP_TAGS, "typenames_from": "the campaign push cores' code (T10SP_TAGS)",
    "corpus": [],
}

PROFILES = {
    "t7": {
        "title": "Black Ops III", "exe_key": "exe.t7", "extractor": "tools/re/treyarch/events.py",
        # pointer pushes whose object id comes from here are weapons (the class-5 id source)
        "object_sources": {0x12DEF20: "weapon"},
        # VM_Notify(inst, ?, object id, event canon, paramcount): found from the canon hashes of
        # corpus event names stored as imm32 (`mov edx, canon("trigger")`) before a call whose
        # callee chain ends here; the wrappers that take a C string hash it with t7.canon
        # (checked: at least one "cstr" wrapper must be found).
        "core": 0x12EC9F0, "core_args": {"inst": 0, "obj": 2, "event": 3, "count": 4},
        "hash": hashes.t7_canon, "hash_family": "t7.canon",
        # scr_const: one init function, SL_GetString(char*) per string (1312 calls)
        "const_init": [0x1C28FE0], "const_alloc": 0x1A77150, "const_width": 4,
        # scrVmPub[inst].top (read off Scr_AddString: add [pub + 0x20], 0x10), stride 0x8a40
        "vmpub_top": 0x5124860, "vmpub_stride": 0x8A40, "value_size": 0x10, "type_off": 8,
        "top_disp": 0x20,
        # object ids an entity push takes: the wrappers' resolver (found) and Scr_GetEntityId,
        # the source Scr_AddEntityNum pushes (it indexes the same per-inst class table by the
        # entref's class, then allocates the id); the push runs name it as the source
        "entity_sources": [0x12DE6A0],
        "corpus": ["t7/decompiled", "t7/source-modtools"],
        "vm_of_inst": {0: "gsc", 1: "csc"},
    },
    "t8": {
        "title": "Black Ops 4", "exe_key": "exe.t8", "extractor": "tools/re/treyarch/events.py",
        # Scr_NotifyNum_Internal(inst, ?, object id, &event): the event is a 64-bit t8.fnv64
        # hash passed by pointer (a 16-byte name copied by the callee). Found from the hashes
        # of corpus event names stored as imm64 before calls to Scr_Notify (0x2774580), whose
        # callee is this; the count is the VM's own (inparamcount), and the payload is the
        # waitresult struct the wrapper pushes last.
        "core": 0x2777430, "core_args": {"inst": 0, "obj": 2, "event": ("deref", 3), "count": None},
        "hash": hashes.t8_fnv64, "hash_family": "t8.fnv64", "hash_mask": 0x7FFFFFFFFFFFFFFF,
        "hash_digits": 16, "require_cstr": False, "name_lists": True,
        "payload": "struct", "waittill_any": True,
        "corpus": ["t8/decompiled"],
        "vm_of_inst": {0: "gsc", 1: "csc"},
    },
    "t6": {
        "title": "Black Ops II", "exe_key": "exe.t6", "extractor": "tools/re/treyarch/events.py",
        # x86, cdecl. VM_Notify(inst, ?, object id, string id, paramcount): Scr_Notify(ent,
        # scr_const.x, n) (0x63dfb0, 207 callers, found from the reads of the `trigger` slot)
        # calls Scr_NotifyNum (0x7ebeb0), which calls this. Events are 16-bit SL ids; the
        # engine's own strings are the scr_const slots one init fills (734 SL_GetString calls).
        "core": 0x3EBE00, "core_args": {"inst": 0, "obj": 2, "event": 3, "count": 4},
        "hash": None, "require_cstr": False,
        "const_init": [0x278440], "const_alloc": 0x221D80, "const_width": 2,
        # scrVmPub[inst].top: Scr_AddInt does `add dword ptr [esi + 0x2d3c960], 8` (esi = inst
        # * 0x42a8); 8-byte values, the type tag at +0
        "vmpub_top": 0x293C960, "vmpub_stride": 0x42A8, "value_size": 8, "type_off": 0,
        "top_disp": 0x2D3C960, "top_width": "dword",
        # Scr_GetEntityId: the object id Scr_AddEntity(Num) pushes (beside the wrappers'
        # FindEntityId 0x3e6990)
        "entity_sources": [0x3E6A00],
        "corpus": ["t6/decompiled"],
        "vm_of_inst": {0: "gsc", 1: "csc"},
    },
    "t5": {
        "title": "Black Ops", "exe_key": "exe.t5", "extractor": "tools/re/treyarch/events.py",
        # x86. Scr_NotifyNum(inst, ?, entnum, classnum, SL string id, paramcount): it resolves
        # the entity and runs VM_Notify itself; reached from Scr_Notify(ent, str, n) (0x518010,
        # 175 callers, found from the server scr_const slot reads) and the client's (0x641ac0).
        "core": 0x1D270, "core_args": {"inst": 0, "obj": 2, "event": 4, "count": 5},
        "core_recv": "entity", "hash": None, "require_cstr": False,
        # scr_const: the server init (482 SL_GetString calls) and the client's (110, another
        # allocator)
        "const_init": [0x1EA220], "const_alloc": 0x1B120, "const_width": 2,
        "const_extra": [([0x18A5C0], 0x186A60, 0)],
        # scrVmPub[inst].top at 0x32c8748 + inst * 0x4320 (read off the core): 8-byte values,
        # type tag at +4
        "vmpub_top": 0x2EC8748, "vmpub_stride": 0x4320, "value_size": 8, "type_off": 4,
        "top_disp": 0x32C8748, "top_width": "dword",
        # Scr_GetEntityId: the id Scr_AddEntityNum pushes
        "entity_sources": [0xB7050],
        "corpus": ["t5/source", "t5/source-modtools"],
        "vm_of_inst": {0: "gsc", 1: "csc"},
    },
    "t4": {
        "title": "World at War", "exe_key": "exe.t4", "extractor": "tools/re/treyarch/events.py",
        # x86. Scr_NotifyNum(entnum, classnum, SL string id, paramcount) with inst in eax
        # (usercall), 134 callers, found from the `trigger`/`damage` slot reads; it resolves
        # the entity and runs VM_Notify itself. SL_GetString(user, char*): the string is
        # argument 1.
        "core": 0x298CC0, "core_args": {"inst": ("reg", "eax"), "obj": 0, "event": 2, "count": 3},
        "core_recv": "entity", "hash": None, "require_cstr": False,
        "const_init": [0x1641C0], "const_alloc": 0x28DE50, "const_width": 2, "const_str_arg": 1,
        # scrVmPub[inst].top at 0x3bd4710 + inst * 0x4320 (read off the core)
        "vmpub_top": 0x37D4710, "vmpub_stride": 0x4320, "value_size": 8, "type_off": 4,
        "top_disp": 0x3BD4710, "top_width": "dword",
        # Scr_GetEntityId: its result is the id Scr_AddObject pushes right after
        "entity_sources": [0x292520],
        "corpus": ["t4/source"],
        "vm_of_inst": {0: "gsc", 1: "csc"},
    },
    "t9": {
        "title": "Black Ops Cold War", "exe_key": "exe.t9",
        "extractor": "tools/re/treyarch/events.py",
        # Scr_NotifyNum_Internal(inst, ?, entref, ?, event, flag): unlike T8 the 64-bit event
        # hash is passed by value on the stack; found as in T8 (corpus event hashes as imm64 before calls to
        # Scr_Notify 0x1bb60b0, 175 callers, whose callee is this).
        "core": 0x1BB97A0, "core_args": {"inst": 0, "obj": 2, "event": 4, "count": None},
        "hash": hashes.t8_fnv64, "hash_family": "t8.fnv64", "hash_mask": 0x7FFFFFFFFFFFFFFF,
        "hash_digits": 16, "require_cstr": False, "name_lists": True,
        "payload": "struct", "waittill_any": True,
        "corpus": ["t9/decompiled"],
        "vm_of_inst": {0: "gsc", 1: "csc"},
    },
    "t11": {
        "title": "Black Ops 7", "exe_key": "exe.t11",
        "extractor": "tools/re/treyarch/events.py",
        "method_note": T11_NOTE,
        # VM_NotifyCtx(ctx, object id, event, paramcount): ctx is the VM context (VM + 0x10,
        # as the builtins' Scr_GetNumParam takes it), the event the full 64-bit FNV-1a of the
        # name, by value. Found from the corpus event hashes stored as imm64 (443 movabs of
        # 214 distinct hashes): 162 precede calls to Scr_Notify(ent, event, count) 0x7487500,
        # which calls Scr_NotifyNum(ctx, entnum, classnum, event, count) 0xa4e16c0 (the object
        # id resolved by 0xa4cf010), which tail-calls this; Scr_NotifyLevel 0xa4e16b0 (ctx,
        # event, count; the level id at ctx + 0x1c) does too. The core pops the pushed values
        # off the stack top at [[ctx + 0x688]].
        "core": 0xA4E15C0, "core_args": {"inst": 0, "obj": 1, "event": 2, "count": 3},
        "hash": hashes.iw9_xhash, "hash_family": "iw9.xhash", "hash_mask": M64,
        "hash_digits": 16, "require_cstr": False, "name_lists": True,
        # the wrappers read the context as [gScrVm[i]] + 0x10 (0xdb17d50 -> server VM
        # 0xdb1f5e8, 0xdb17d58 -> client VM 0xdb6a460, the VMs the builtin sets select)
        "vm_of_inst": {}, "vm_of_ctx": {0xDB1F5F8: "gsc", 0xDB6A470: "csc"},
        # scr_const: the string-id getter(&out, char*, 0, 1) 0xa4b88b0 (the tag-2 push core's)
        # called 2351 + 346 + 45 times by three inits, each id then stored in a global; the
        # SL wrappers (0x7487530...) turn an id back into its text and hash that
        "const_init": [0x75D5BB0, 0x75EA580, 0x75ED560], "const_alloc": 0xA4B88B0,
        "const_width": 4, "const_str_arg": 1, "const_out_arg": 0,
        # pushers: `add qword ptr [top], 0x10` then a byte tag at +8 (T10_TAGS)
        "vmpub_top": None, "vmpub_ctx": {"ctxs": [0xDB1F5F8, 0xDB6A470], "top_off": 0x688},
        "top_disp": None, "value_size": 0x10, "type_off": 8, "tag_width": "byte",
        "insts": (0xDB1F5F8, 0xDB6A470),
        "typenames": T10_TAGS, "typenames_from": "the push cores' code (T10_TAGS)",
        # entity ids: FindEntityId 0xa4cf010 (the wrappers') and Scr_GetEntityId 0xa4d2640
        # (Scr_AddEntityNum 0xa4e3500 pushes its result through the tag-1 core 0xa4e3a70)
        "entity_sources": [0xA4CF010, 0xA4D2640],
        "corpus": ["@bo7_corpus"],
    },
    "t10": {
        "title": "Black Ops 6", "exe_key": "exe.t10",
        "extractor": "tools/re/treyarch/events.py",
        "method_note": T10_NOTE,
        # the multiplayer dump; events are 32-bit string ids (how both cores were found:
        # reports/events_t10.md). VM_NotifyVm(vm, object id, event, count), BO7's core, behind
        # Scr_Notify 0x59a3400; VM_NotifyCtx(ctx = vm + 0x30, event, object id, count, ...)
        # behind 0x81d7630 / 0x81d75c0 / 0x81d77d0.
        "core": 0x81E2CD0, "core_args": {"inst": 0, "obj": 1, "event": 2, "count": 3},
        "extra_cores": [(0x81D7660, {"inst": 0, "obj": 2, "event": 1, "count": 3})],
        "hash": None, "require_cstr": False,
        # scr_const: getter(&out, char*, 0x20, 1) 0x81c1480 called 2501 + 382 + 59 times by
        # three inits (the strings in clear), each id then stored in a global
        "const_init": [0x5AAF0C0, 0x5AC4FB0, 0x5AC84A0], "const_alloc": 0x81C1480,
        "const_width": 4, "const_str_arg": 1, "const_out_arg": 0,
        "vm_of_inst": {}, "vm_of_ctx": {0xB27D730: "gsc", 0xB27D760: "gsc"},
        # pushers: the top at ctx + 0xc478, advanced by a store (`lea rax, [rcx + 0x10]; mov
        # [ctx + 0xc478], rax`), a byte tag at +8 (T10_TAGS); the server context only (the
        # client VM is not located in this dump)
        "vmpub_top": 0xB27D760 + 0xC478, "vmpub_stride": 0, "insts": (0,),
        "top_disp": 0xC478, "top_store": True, "value_size": 0x10, "type_off": 8,
        "tag_width": "byte",
        "typenames": T10_TAGS, "typenames_from": "the push cores' code (T10_TAGS)",
        # no Black Ops 6 script corpus: the coverage block counts nothing; the report measures
        # the catalogue against the Black Ops 7 corpus instead (cross_corpus)
        "corpus": [],
        "cross_corpus": {"roots": ["@bo7_corpus"], "hash": hashes.iw9_xhash,
                         "label": "Black Ops 7 corpus (bo7_corpus), names hashed with iw9.xhash"},
        "merge": [("t10sp", T10SP)],
    },
    "t10sp": T10SP,
}


if __name__ == "__main__":
    R.main(PROFILES)
