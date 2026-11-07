// question: `&spawnstruct` when the current namespace defines spawnstruct: the script function or the builtin?
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
	f = ::spawnstruct;
	r = [[ f ]]();
	logprint("result=" + r);
}
