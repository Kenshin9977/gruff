"""Unicorn harness for an Image (x86 or x64): the image mapped at its own base, a stack, a fake
TEB/PEB (x64: gs:[0x30], gs:[0x60]; x86: fs:[0x18], fs:[0x30]), a bump heap, and call(va, args)
that runs one function to its return. Pattern from graywolf-tools zonekit/emulator.py (the user's
GPL-3 code), reduced to what the extractions need.

Hook helpers work for both modes: `arg(i)` reads the i-th argument at a function's entry (x64:
rcx, rdx, r8, r9, then the stack; x86: [esp + 4 + 4i], cdecl/stdcall), `ret(rax, pop)` returns
from a hooked function at its entry (pop: bytes a stdcall callee removes).
"""
import struct

from .image import user_site

user_site()
import unicorn  # noqa: E402
from unicorn import x86_const as X  # noqa: E402

STACK = 0x7F0000000000
STACK_SZ = 0x200000
TEB = 0x7E0000000000
HEAP = 0x7D0000000000
HEAP_SZ = 0x4000000
RET = 0x7C0000000000  # sentinel return address, mapped, never executed
# x86: everything below 4 GiB
STACK32, TEB32, HEAP32, RET32 = 0x0E000000, 0x0D000000, 0x10000000, 0x0C000000
GDT32 = 0x0B000000
# x86 images reaching past 0x0B000000 (IW3's 223 MB file layout): the same regions moved higher
HIGH32 = (0x20000000, 0x21000000, 0x22000000, 0x26000000, 0x27000000)

ARGREGS64 = [X.UC_X86_REG_RCX, X.UC_X86_REG_RDX, X.UC_X86_REG_R8, X.UC_X86_REG_R9]


def gdt_entry(base, limit, access, flags):
    """One 8-byte x86 segment descriptor."""
    return struct.pack("<Q", (limit & 0xFFFF) | ((base & 0xFFFFFF) << 16) | ((access & 0xFF) << 40)
                       | (((limit >> 16) & 0xF) << 48) | ((flags & 0xF) << 52)
                       | (((base >> 24) & 0xFF) << 56))


def write_teb(uc, teb, x64):
    """The fake TEB's self pointer, PEB pointer (teb + 0x10000) and TLS slot array (teb +
    0x8000); x64: the GS base too (x86 reaches FS through a GDT entry the caller sets)."""
    if x64:
        uc.mem_write(teb + 0x30, struct.pack("<Q", teb))
        uc.mem_write(teb + 0x60, struct.pack("<Q", teb + 0x10000))
        uc.mem_write(teb + 0x58, struct.pack("<Q", teb + 0x8000))  # TLS slots
        uc.msr_write(0xC0000101, teb)  # GS base
    else:
        uc.mem_write(teb + 0x18, struct.pack("<I", teb))
        uc.mem_write(teb + 0x30, struct.pack("<I", teb + 0x10000))
        uc.mem_write(teb + 0x2C, struct.pack("<I", teb + 0x8000))


class MemoryAccess:
    """Heap and memory helpers shared by the emulators: `self.uc`, `self.x64`, `self.heap0`
    and `self.heap_next` are the host's."""

    def alloc(self, n, align=16):
        a = (self.heap_next + align - 1) & ~(align - 1)
        self.heap_next = a + n
        assert self.heap_next < self.heap0 + HEAP_SZ
        return a

    def r32(self, va):
        return struct.unpack("<I", self.uc.mem_read(va, 4))[0]

    def r64(self, va):
        return struct.unpack("<Q", self.uc.mem_read(va, 8))[0]

    def rptr(self, va):
        return self.r64(va) if self.x64 else self.r32(va)

    def w32(self, va, v):
        self.uc.mem_write(va, struct.pack("<I", v & 0xFFFFFFFF))

    def wptr(self, va, v):
        if self.x64:
            self.uc.mem_write(va, struct.pack("<Q", v & 0xFFFFFFFFFFFFFFFF))
        else:
            self.w32(va, v)


class Emu(MemoryAccess):
    def __init__(self, img, null_page=False):
        self.img = img
        self.x64 = img.x64
        uc = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_64 if self.x64 else unicorn.UC_MODE_32)
        size = (img.size + 0xFFFF) & ~0xFFFF
        uc.mem_map(img.base, size, unicorn.UC_PROT_ALL)
        uc.mem_write(img.base, img.d)
        if self.x64:
            self.stack, self.teb, self.heap0, self.retaddr = STACK, TEB, HEAP, RET
        else:
            self.stack, self.teb, self.heap0, self.retaddr = STACK32, TEB32, HEAP32, RET32
            self.gdt = GDT32
            if img.base + size > GDT32:
                self.stack, self.teb, self.heap0, self.retaddr, self.gdt = HIGH32
        uc.mem_map(self.stack, STACK_SZ)
        uc.mem_map(self.teb, 0x20000)
        uc.mem_map(self.heap0, HEAP_SZ)
        uc.mem_map(self.retaddr, 0x1000)
        write_teb(uc, self.teb, self.x64)
        if not self.x64:
            _set_fs32(uc, self.teb, self.gdt)
        self.uc = uc
        self.heap_next = self.heap0
        self.pages = 0
        self.page_cap = 256
        if null_page:
            uc.mem_map(0, 0x100000)  # a null pointer plus an offset reads zeros

    # --- memory ---------------------------------------------------------------------------------
    def map_on_unmapped(self, cap=256):
        """Map a zero page wherever an unmapped read or write lands (at most `cap` per run; reset
        `self.pages` before each run)."""
        self.page_cap = cap

        def on_unmapped(uc, access, addr, size, value, ud):
            if self.pages > self.page_cap:
                return False
            try:
                uc.mem_map(addr & ~0xFFF, 0x1000)
            except unicorn.UcError:
                return False
            self.pages += 1
            return True
        return self.uc.hook_add(unicorn.UC_HOOK_MEM_UNMAPPED, on_unmapped)

    # --- registers and calls ----------------------------------------------------------------------
    @property
    def sp_reg(self):
        return X.UC_X86_REG_RSP if self.x64 else X.UC_X86_REG_ESP

    @property
    def ip_reg(self):
        return X.UC_X86_REG_RIP if self.x64 else X.UC_X86_REG_EIP

    @property
    def ax_reg(self):
        return X.UC_X86_REG_RAX if self.x64 else X.UC_X86_REG_EAX

    def ip(self):
        return self.uc.reg_read(self.ip_reg)

    def arg(self, i):
        """The i-th argument at a function's entry (return address on top of the stack)."""
        uc = self.uc
        sp = uc.reg_read(self.sp_reg)
        if self.x64:
            if i < 4:
                return uc.reg_read(ARGREGS64[i])
            return self.r64(sp + 8 + 8 * i)
        return self.r32(sp + 4 + 4 * i)

    def arg32(self, i):
        return self.arg(i) & 0xFFFFFFFF

    def ret(self, rax=None, pop=0):
        """Return from the function whose entry is being executed (in a code hook)."""
        uc = self.uc
        sp = uc.reg_read(self.sp_reg)
        uc.reg_write(self.ip_reg, self.rptr(sp))
        uc.reg_write(self.sp_reg, sp + self.img.ptr + pop)
        if rax is not None:
            uc.reg_write(self.ax_reg, rax)

    def call(self, va, args=(), count=0, timeout_us=0, regs=None):
        """Run the function at `va` with `args` (x64 Microsoft ABI; x86 every argument on the
        stack, cdecl/stdcall) until it returns. `regs` {unicorn reg: value} set besides (x86
        thiscall/fastcall: ecx, edx)."""
        uc = self.uc
        sp = self.stack + STACK_SZ - 0x1000
        if self.x64:
            for r, v in zip(ARGREGS64, args):
                uc.reg_write(r, v)
            sp -= 0x20 + 8 * max(0, len(args) - 4)  # home space + stack args
            for k, v in enumerate(args[4:]):
                uc.mem_write(sp + 0x20 + 8 * k, struct.pack("<Q", v & (1 << 64) - 1))
            sp -= 8
            uc.mem_write(sp, struct.pack("<Q", self.retaddr))
        else:
            sp -= 4 * len(args)
            for k, v in enumerate(args):
                uc.mem_write(sp + 4 * k, struct.pack("<I", v & 0xFFFFFFFF))
            sp -= 4
            uc.mem_write(sp, struct.pack("<I", self.retaddr))
        for r, v in (regs or {}).items():
            uc.reg_write(r, v)
        uc.reg_write(self.sp_reg, sp)
        uc.emu_start(va, self.retaddr, timeout=timeout_us, count=count)
        return uc.reg_read(self.ax_reg)

    def run(self, va, args=(), count=2_000_000, regs=None):
        """call() that reports a fault instead of raising: (rax or None, error or None)."""
        try:
            return self.call(va, args, count=count, regs=regs), None
        except unicorn.UcError as ex:
            return None, "%s at %#x" % (ex, self.ip())


def _set_fs32(uc, base, gdt=GDT32):
    """x86: point FS at `base` through a GDT entry (unicorn has no FS base MSR in 32-bit mode)."""
    uc.mem_map(gdt, 0x1000)
    uc.mem_write(gdt + 8 * 1, gdt_entry(0, 0xFFFFF, 0x9B, 0xC))   # code, ring 0
    uc.mem_write(gdt + 8 * 2, gdt_entry(0, 0xFFFFF, 0x93, 0xC))   # data
    uc.mem_write(gdt + 8 * 3, gdt_entry(base, 0xFFFFF, 0x93, 0xC))  # fs
    uc.reg_write(X.UC_X86_REG_GDTR, (0, gdt, 0x1000, 0))
    uc.reg_write(X.UC_X86_REG_SS, 2 << 3)
    uc.reg_write(X.UC_X86_REG_FS, 3 << 3)
