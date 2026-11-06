"""A believable `self` for the T7 handler runs.

A method handler gets (inst, &entref); entref {u32 entnum, ..., u16 classnum at +8} names the
object. With a null entref every method ran on entity 0 of class 0 whose memory is all zeros, so
a handler that checks what its entity is ("not an actor", "Not a sentient", "entity must be a
player entity", "not a hud element") raised Scr_Error before it read its parameters (563 runs).

What is built here, from the game's own code:

- g_entities[0]: the entref resolver (the "not an entity" function a method calls on its entref)
  is run once unhooked on entref {0, class 0}; it returns entity 0's address (the base pointer is
  obfuscated: the resolver decodes it, we do not).
- The entity's sub-object pointers: every check site of a "not an actor" / "is not a player" /
  "must be a player entity" / "not a client" / "not a vehicle" / "not a sentient" message loads or
  compares a qword at [entity + OFF] just before the message; those offsets (with the number of
  sites) are collected and each slot of entity 0 gets a pointer to its own zeroed buffer.
- Variants of the entref: class 0 (entity 0, populated as above) and class 1 (hud element 0).
  A method is run with each variant and the run that read the most parameters (then: returned)
  is kept, with the variant recorded.

Entity parameters (Scr_GetEntity(index)) return the same populated entity 0.
"""
import collections
import re
import struct

from common.emu import Emu, unicorn

CHECK_MESSAGES = re.compile(
    r"(?i)(not an? (actor|player|client|vehicle|sentient|ai)\b|must be (a |called on )?(a )?player"
    r"|non-player|is not a player|not a player entity|entity not a vehicle)")
SLOT = re.compile(r"qword ptr \[(\w+)(?: \+ \w+)? \+ (0x[0-9a-f]+)\]")
ENTITY_SIZE_MAX = 0x800
SUB_SIZE = 0x40000


class FakeSelf:
    def __init__(self, img, log, resolver_rva=None, entity_size=0x4F8):
        self.img = img
        self.log = log
        self.resolver_rva = resolver_rva
        self.slots = self.pointer_slots()
        self.entity_size = entity_size
        self.variant = "entity"
        self.used = collections.Counter()
        self.ent0 = None

    def pointer_slots(self):
        """{offset: sites} of the qword the entity checks test before raising a kind message."""
        img = self.img
        msgs = {}
        for name, sva, vsz, ch in img.sections:
            if name not in (".rdata", ".data"):
                continue
            blob = img.d[sva:sva + vsz]
            for m in re.finditer(rb"[\x20-\x7e]{6,120}\0", blob):
                s = m.group()[:-1].decode("latin1")
                if CHECK_MESSAGES.search(s):
                    # a string may follow other bytes without a NUL ("\x80?Not a sentient" is
                    # referenced from the N): register the first few start positions
                    for k in range(4):
                        msgs[img.base + sva + m.start() + k] = s[k:]
        slots = collections.Counter()
        refs = img.code_refs_many(msgs)
        for va in msgs:
            for ref in refs[va]:
                f = img.func(ref)
                if not f:
                    continue
                start = max(f[0], ref - 0x30)
                prev = []
                for ins in img.md.disasm(img.d[start - img.base:ref - img.base + 8], start):
                    if ins.address >= ref:
                        break
                    prev.append(ins)
                for ins in prev[-5:]:
                    m = SLOT.search(ins.op_str)
                    if m and ins.mnemonic in ("cmp", "mov") and m.group(1) not in ("rsp", "rbp", "rip"):
                        off = int(m.group(2), 16)
                        if 0x8 <= off < ENTITY_SIZE_MAX and off % 8 == 0:
                            slots[off] += 1
        self.log.append("fake self: %d check messages, pointer slots %s" % (
            len(msgs), ", ".join("%#x(%d)" % kv for kv in sorted(slots.items()))))
        return slots

    def attach(self, runner):
        """Find entity 0 by running the resolver unhooked, then populate its pointer slots."""
        img = self.img
        emu = runner.emu
        if self.resolver_rva is None:
            if len(runner.res) != 1:
                raise SystemExit("fake self: expected one entref resolver, got %s" % [
                    hex(r - img.base) for r in runner.res])
            self.resolver_rva = next(iter(runner.res)) - img.base
        probe = Emu(img, null_page=True)
        probe.map_on_unmapped()
        ref = probe.alloc(16)
        probe.uc.mem_write(ref, b"\0" * 16)
        r, err = probe.run(img.base + self.resolver_rva, (ref,), count=200_000)
        if err or r is None:
            raise SystemExit("fake self: the resolver faulted on entref {0, 0}: %s" % err)
        # In the dump the entity array is not allocated (its obfuscated base decodes to 0): entity
        # 0 is then at address 0, which every resolver that indexes the array reaches. Entity 0 is
        # populated there (the null page) and in a heap copy that the hooked resolver and the
        # entity parameter accessor return.
        homes = [r]  # 0 included: the null page is mapped
        heap_ent = emu.alloc(ENTITY_SIZE_MAX)
        emu.uc.mem_write(heap_ent, b"\0" * ENTITY_SIZE_MAX)
        homes.append(heap_ent)
        for off in sorted(self.slots):
            buf = emu.alloc(SUB_SIZE)
            emu.uc.mem_write(buf, b"\0" * SUB_SIZE)
            for h in homes:
                emu.wptr(h + off, buf)
        self.ent0 = heap_ent
        self.entref = emu.alloc(16)
        runner.entity_value = heap_ent
        self.log.append("fake self: the resolver (rva %#x) maps entref {0, 0} to %s; %d pointer "
                        "slots filled" % (self.resolver_rva, "address 0 (entity array not "
                                          "allocated in the dump)" if r == 0 else "%#x" % r,
                                          len(self.slots)))

    def provide(self, runner, inst, method):
        """(entref pointer, resolver return value) for the current variant."""
        cls = {"entity": 0, "hudelem": 1}[self.variant]
        runner.emu.uc.mem_write(self.entref, struct.pack("<IIHH", 0, 0, cls, 0) + b"\0" * 4)
        return (self.entref if method else 0), self.ent0

    def best_profile(self, profile_fn, method):
        """Run `profile_fn()` with each variant (methods) and keep the best run."""
        variants = ["entity", "hudelem"] if method else ["entity"]
        best = None
        for v in variants:
            self.variant = v
            hp = profile_fn()
            score = (len(hp["types"]), hp["stop_at_max"] == "returned")
            if best is None or score > best[0]:
                best = (score, v, hp)
        self.variant = "entity"
        hp = dict(best[2], self_variant=best[1])
        self.used[best[1]] += 1
        return hp

    def summary(self):
        return ["fake self: variant kept per handler run %s" % dict(self.used)]
