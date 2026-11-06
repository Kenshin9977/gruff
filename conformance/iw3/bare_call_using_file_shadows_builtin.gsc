// question: a bare call to a name that is a builtin and also a function of an #include'd file: which one runs?
// with: zz_bare_lib.gsc
// expect: f0 T0 bare done
// fact: a function of the same name in an #include'd file does not shadow the builtin for a bare call: the builtin runs
// evidence: static: the call emitter (0x50ad90) only searches the current file's own functions (0x50ac10) before the builtins; #include'd files come after
// vs t4: same
#include zz\zz_bare_lib;

main()
{
	a = spawnstruct();
	logprint("bare done");
}
