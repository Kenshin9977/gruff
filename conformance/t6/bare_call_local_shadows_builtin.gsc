// question: a bare call to a name that is both a builtin (spawnstruct) and a function of the same file: which one runs?
// fact: a bare call reaches the builtin even when the file defines a function of the same name (the linker tries the builtins first); a file-qualified call (path::f) reaches the script function
// vs t7: the current namespace's function first, then the builtin
// evidence: static: the import linker (0x7e08b0) takes an import with an empty namespace (the encoding of a bare call, by gsc-tool and by Treyarch's own compiler: 36 calls to functions of the same file in the retail common_mp zone use it) to the builtin lookups first (0x636f80 functions, 0x639530 methods) and only when no builtin has the name to the script resolver 0x7df5f0 (the object itself, then its includes); a non-empty namespace goes to the script resolver only
// expect: f0 T0 bare is int=0
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 qualified=42

spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

main()
{
	a = spawnstruct();
	logprint("bare is int=" + isint(a));
	b = maps\mp\zz_harness\bare_call_local_shadows_builtin::spawnstruct();
	logprint("qualified=" + b);
}
