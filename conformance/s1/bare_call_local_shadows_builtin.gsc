// question: a bare call to a name that is both a builtin (spawnstruct) and a function of the same file: which one runs, and does the path-qualified call reach the script function?
// unsupported: no measure: the compiler (gsc-tool) refuses a script function named like a builtin, and S1's own compiler (which decides bare-call resolution at compile time: its bytecode calls a builtin by id and a script function by code position) is not available
// expect compile error: function name 'spawnstruct' already defined as builtin
// vs iw6: not comparable (not measured in either)

spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

main()
{
	a = spawnstruct();
	logprint("bare done");
	b = zz\bare_call_local_shadows_builtin::spawnstruct();
	logprint("qualified=" + b);
}
