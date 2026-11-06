// question: a bare call to a name that is a builtin and also a function of an #include'd file: which one runs?
// with: zz_bare_lib.gsc
// fact: the builtin: a function of an #include'd file never shadows a builtin for a bare call
// vs t6: same
// expect: f0 T0 is struct=1

#include zz_harness\zz_bare_lib;

main()
{
	a = spawnstruct();
	logprint("is struct=" + (isdefined(a) && !isint(a)));
}
