// question: a bare call to a name that is a builtin and also a function of an #include'd file: which one runs?
// with: zz_bare_lib.gsc
// expect: f0 T0 bare done
// vs t7: same: a function of a #using'd file does not shadow the builtin
// fact: a function of the same name in an #include'd file does not shadow the builtin for a bare call: the builtin runs
// evidence: static: the call emitter (0x682ca0) only searches the current file's own functions before the builtins; #include'd files come after
#include zz\zz_bare_lib;

main()
{
	a = spawnstruct();
	iprintln("bare done");
}
