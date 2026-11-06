// question: a bare call to a name that is a builtin and also a function of an #include'd file: which one runs?
// with: zz_bare_lib.gsc
// fact: the builtin: a function of an #include'd file never shadows a builtin for a bare call
// vs t7: same
// evidence: static: the import linker (0x7e08b0) takes an import with an empty namespace (the encoding of a bare call, by gsc-tool and by Treyarch's own compiler: 36 calls to functions of the same file in the retail common_mp zone use it) to the builtin lookups first (0x636f80 functions, 0x639530 methods) and only when no builtin has the name to the script resolver 0x7df5f0 (the object itself, then its includes); a non-empty namespace goes to the script resolver only
// expect: f0 T0 is struct=1

#include maps\mp\zz_harness\zz_bare_lib;

main()
{
	a = spawnstruct();
	logprint("is struct=" + (isdefined(a) && !isint(a)));
}
