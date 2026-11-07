// question: a bare call to a name that is a builtin and also a function of an #include'd file: which one runs?
// unsupported: no measure: the compiler (gsc-tool) refuses a script function named like a builtin, and IW7's own compiler (which decides bare-call resolution at compile time: its bytecode calls a builtin by id and a script function by code position) is not available
// expect compile error: function name 'spawnstruct' already defined as builtin
// with: zz_bare_lib.gsc
// vs s1: not comparable (not measured in either)
#include zz\zz_bare_lib;

main()
{
	a = spawnstruct();
	logprint("bare done");
}
