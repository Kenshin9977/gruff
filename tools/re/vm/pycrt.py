"""The C runtime imports of a dynamically linked x64 build (VCRUNTIME140, the UCRT api sets) done
in Python for a vmcore.GameVM: the import stubs otherwise return 0 without doing anything.

Only what the script VM's paths call is covered; an unknown CRT import still returns 0 and is
logged by name (the run's `stubs`). setjmp/longjmp keep the MSVC x64 _JUMP_BUFFER layout.
"""
import struct

from common.emu import X

JB = ((0x08, "RBX"), (0x18, "RBP"), (0x20, "RSI"), (0x28, "RDI"), (0x30, "R12"), (0x38, "R13"),
      (0x40, "R14"), (0x48, "R15"))


def _reg(vm, n):
    return vm.uc.reg_read(getattr(X, "UC_X86_REG_" + n))


def _setreg(vm, n, v):
    vm.uc.reg_write(getattr(X, "UC_X86_REG_" + n), v)


def _bytes(vm, a, n):
    return bytes(vm.uc.mem_read(a, n)) if n else b""


def _cbytes(vm, a, limit=1 << 20):
    s = vm.cstr(a, limit)
    return (s or "").encode("latin-1")


def setjmp(vm):
    buf = vm.emu.arg(0)
    sp = _reg(vm, "RSP")
    for off, r in JB:
        vm.w64(buf + off, _reg(vm, r))
    vm.w64(buf + 0x10, sp + 8)
    vm.w64(buf + 0x50, vm.r64(sp))
    for i in range(10):
        v = vm.uc.reg_read(getattr(X, "UC_X86_REG_XMM%d" % (6 + i)))
        vm.uc.mem_write(buf + 0x60 + 0x10 * i, struct.pack("<QQ", v & (1 << 64) - 1, v >> 64))
    return 0


def longjmp(vm):
    buf, val = vm.emu.arg(0), vm.emu.arg(1) & 0xFFFFFFFF
    for off, r in JB:
        _setreg(vm, r, vm.r64(buf + off))
    for i in range(10):
        lo, hi = struct.unpack("<QQ", _bytes(vm, buf + 0x60 + 0x10 * i, 16))
        vm.uc.reg_write(getattr(X, "UC_X86_REG_XMM%d" % (6 + i)), lo | hi << 64)
    sp = vm.r64(buf + 0x10) - 8
    vm.w64(sp, vm.r64(buf + 0x50))
    _setreg(vm, "RSP", sp)   # the stub's `ret` then lands on the saved rip
    return val or 1


def memcpy(vm):
    d, s, n = vm.emu.arg(0), vm.emu.arg(1), vm.emu.arg(2)
    if n:
        vm.uc.mem_write(d, _bytes(vm, s, n))
    return d


def memset(vm):
    d, c, n = vm.emu.arg(0), vm.emu.arg(1) & 0xFF, vm.emu.arg(2)
    while n > 0:
        k = min(n, 1 << 20)
        vm.uc.mem_write(d, bytes([c]) * k)
        d += k
        n -= k
    return vm.emu.arg(0)


def memcmp(vm):
    n = vm.emu.arg(2)
    a, b = _bytes(vm, vm.emu.arg(0), n), _bytes(vm, vm.emu.arg(1), n)
    return 0 if a == b else (1 if a > b else (1 << 64) - 1)


def memchr(vm):
    p, c, n = vm.emu.arg(0), vm.emu.arg(1) & 0xFF, vm.emu.arg(2)
    i = _bytes(vm, p, n).find(bytes([c]))
    return p + i if i >= 0 else 0


def strlen(vm):
    return len(_cbytes(vm, vm.emu.arg(0)))


def _cmp(a, b):
    return 0 if a == b else (1 if a > b else (1 << 64) - 1)


def strcmp(vm):
    return _cmp(_cbytes(vm, vm.emu.arg(0)), _cbytes(vm, vm.emu.arg(1)))


def strncmp(vm):
    n = vm.emu.arg(2)
    return _cmp(_cbytes(vm, vm.emu.arg(0))[:n], _cbytes(vm, vm.emu.arg(1))[:n])


def stricmp(vm):
    return _cmp(_cbytes(vm, vm.emu.arg(0)).lower(), _cbytes(vm, vm.emu.arg(1)).lower())


def strnicmp(vm):
    n = vm.emu.arg(2)
    return _cmp(_cbytes(vm, vm.emu.arg(0)).lower()[:n], _cbytes(vm, vm.emu.arg(1)).lower()[:n])


def strchr(vm):
    p, c = vm.emu.arg(0), vm.emu.arg(1) & 0xFF
    s = _cbytes(vm, p) + b"\0"
    i = s.find(bytes([c]))
    return p + i if i >= 0 else 0


def strrchr(vm):
    p, c = vm.emu.arg(0), vm.emu.arg(1) & 0xFF
    s = _cbytes(vm, p) + b"\0"
    i = s.rfind(bytes([c]))
    return p + i if i >= 0 else 0


def strstr(vm):
    p = vm.emu.arg(0)
    i = _cbytes(vm, p).find(_cbytes(vm, vm.emu.arg(1)))
    return p + i if i >= 0 else 0


def strncpy(vm):
    d, s, n = vm.emu.arg(0), vm.emu.arg(1), vm.emu.arg(2)
    b = _cbytes(vm, s)[:n]
    vm.uc.mem_write(d, b + bytes(n - len(b)))
    return d


def strcpy_s(vm):
    d, n, s = vm.emu.arg(0), vm.emu.arg(1), vm.emu.arg(2)
    b = _cbytes(vm, s)[:max(n, 1) - 1]
    vm.uc.mem_write(d, b + b"\0")
    return 0


def strncpy_s(vm):
    d, n, s, c = vm.emu.arg(0), vm.emu.arg(1), vm.emu.arg(2), vm.emu.arg(3)
    b = _cbytes(vm, s)[:min(c, max(n, 1) - 1)]
    vm.uc.mem_write(d, b + b"\0")
    return 0


def strcat_s(vm):
    d, n, s = vm.emu.arg(0), vm.emu.arg(1), vm.emu.arg(2)
    b = (_cbytes(vm, d) + _cbytes(vm, s))[:max(n, 1) - 1]
    vm.uc.mem_write(d, b + b"\0")
    return 0


def _ch(f):
    return lambda vm: f(vm.emu.arg(0) & 0xFF)


def atoi(vm):
    s = _cbytes(vm, vm.emu.arg(0)).strip()
    num = b""
    for i, c in enumerate(s):
        if chr(c).isdigit() or (i == 0 and c in b"+-"):
            num += bytes([c])
        else:
            break
    try:
        return int(num) & (1 << 64) - 1
    except ValueError:
        return 0


def vsprintf(vm):
    """__stdio_common_vsprintf(options, buf, size, fmt, locale, va_list)."""
    buf, size, fmt, va = vm.emu.arg(1), vm.emu.arg(2), vm.emu.arg(3), vm.emu.arg(5)
    text = vm.format(vm.cstr(fmt, 4096) or "", lambda k: vm.r64(va + 8 * k))
    b = text.encode("latin-1", "replace")
    if size and buf:
        n = min(len(b), size - 1)
        vm.uc.mem_write(buf, b[:n] + b"\0")
    return len(b) if len(b) < size or not size else (1 << 64) - 1


def vsnprintf_s(vm):
    """__stdio_common_vsnprintf_s(options, buf, size, maxcount, fmt, locale, va_list)."""
    buf, size, fmt, va = vm.emu.arg(1), vm.emu.arg(2), vm.emu.arg(4), vm.emu.arg(6)
    text = vm.format(vm.cstr(fmt, 4096) or "", lambda k: vm.r64(va + 8 * k))
    b = text.encode("latin-1", "replace")
    if size and buf:
        n = min(len(b), size - 1)
        vm.uc.mem_write(buf, b[:n] + b"\0")
    return len(b) if len(b) < size else (1 << 64) - 1


def malloc(vm):
    return vm._heap_alloc(vm.emu.arg(0))


def calloc(vm):
    n = vm.emu.arg(0) * vm.emu.arg(1)
    a = vm._heap_alloc(n)
    return a


def realloc(vm):
    return vm._heap_realloc(vm.emu.arg(0), vm.emu.arg(1))


def aligned_malloc(vm):
    return vm.big_alloc(max(vm.emu.arg(0), 1), max(vm.emu.arg(1), 16))


def _math1(f):
    """double f(double): the argument and the result in xmm0."""
    import math

    def h(vm):
        x = struct.unpack("<d", struct.pack("<Q", vm.uc.reg_read(X.UC_X86_REG_XMM0)
                                            & (1 << 64) - 1))[0]
        r = getattr(math, f)(x)
        vm.uc.reg_write(X.UC_X86_REG_XMM0, struct.unpack("<Q", struct.pack("<d", r))[0])
        return 0
    return h


def _math1f(f):
    """float f(float) (floorf...): xmm0's low single."""
    import math

    def h(vm):
        v = vm.uc.reg_read(X.UC_X86_REG_XMM0)
        x = struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]
        r = getattr(math, f)(x)
        vm.uc.reg_write(X.UC_X86_REG_XMM0, struct.unpack("<I", struct.pack("<f", r))[0])
        return 0
    return h


HANDLERS = {
    "floor": _math1("floor"), "ceil": _math1("ceil"), "sqrt": _math1("sqrt"),
    "floorf": _math1f("floor"), "ceilf": _math1f("ceil"), "sqrtf": _math1f("sqrt"),
    "__intrinsic_setjmp": setjmp, "_setjmp": setjmp, "longjmp": longjmp,
    "memcpy": memcpy, "memmove": memcpy, "memset": memset, "memcmp": memcmp, "memchr": memchr,
    "strlen": strlen, "strcmp": strcmp, "strncmp": strncmp, "_stricmp": stricmp,
    "_strnicmp": strnicmp, "strchr": strchr, "strrchr": strrchr, "strstr": strstr,
    "strncpy": strncpy, "strcpy_s": strcpy_s, "strncpy_s": strncpy_s, "strcat_s": strcat_s,
    "tolower": _ch(lambda c: ord(chr(c).lower()) if c < 128 else c),
    "toupper": _ch(lambda c: ord(chr(c).upper()) if c < 128 else c),
    "isdigit": _ch(lambda c: int(48 <= c <= 57)),
    "isspace":_ch(lambda c: int(c in b" \t\n\r\v\f")),
    "isalpha": _ch(lambda c: int(chr(c).isalpha() and c < 128)),
    "isalnum": _ch(lambda c: int(chr(c).isalnum() and c < 128)),
    "isupper": _ch(lambda c: int(65 <= c <= 90)),
    "atoi": atoi, "__stdio_common_vsprintf": vsprintf,
    "__stdio_common_vsnprintf_s": vsnprintf_s,
    "malloc": malloc, "calloc": calloc, "realloc": realloc, "free": lambda vm: 0,
    "_aligned_malloc": aligned_malloc, "_aligned_free": lambda vm: 0,
}


def install(vm):
    vm.import_handlers.update(HANDLERS)
