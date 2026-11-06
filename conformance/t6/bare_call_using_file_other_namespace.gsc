// question: a bare call to a function that only an #include'd file defines: does the game's linker find it?
// with: zz_bare_lib.gsc
// fact: a bare call reaches a function of an #include'd file (T6 has no namespaces: the script resolver searches the object's includes)
// vs t7: a bare call never reaches another namespace, even #using'd: unresolved external
// evidence: static: the import linker (0x7e08b0) takes an import with an empty namespace (the encoding of a bare call, by gsc-tool and by Treyarch's own compiler: 36 calls to functions of the same file in the retail common_mp zone use it) to the builtin lookups first (0x636f80 functions, 0x639530 methods) and only when no builtin has the name to the script resolver 0x7df5f0 (the object itself, then its includes); a non-empty namespace goes to the script resolver only
// expect: f0 T0 lib_only=7

#include maps\mp\zz_harness\zz_bare_lib;

main()
{
	a = lib_only();
	logprint("lib_only=" + a);
}
