// question: `::spawnstruct` when the file defines spawnstruct: the script function or the builtin?
// fact: the builtin, as for a bare call (builtins first), with the same broken result as func_ref_builtin_only: the script function is not reached
// vs t7: the current namespace's script function first
// evidence: static: the import linker (0x7e08b0) takes an import with an empty namespace (the encoding of a bare call, by gsc-tool and by Treyarch's own compiler: 36 calls to functions of the same file in the retail common_mp zone use it) to the builtin lookups first (0x636f80 functions, 0x639530 methods) and only when no builtin has the name to the script resolver 0x7df5f0 (the object itself, then its includes); a non-empty namespace goes to the script resolver only
// unsupported: no meaningful golden: the builtin's machine code runs as bytecode (see func_ref_builtin_only)
// expect: f0 T0 script error: getdvar* expecting string or hash, given precodepos
// expect: f0 T0 script error: pair 'result=' and 'undefined' has unmatching types 'string' and 'undefined'
// expect: f0 T0 undefined

spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

main()
{
	f = ::spawnstruct;
	r = [[ f ]]();
	logprint("result=" + r);
}
