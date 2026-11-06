// question: a bare call to a function that only an #include'd file defines: is it found?
// with: zz_bare_lib.gsc
// fact: yes: a bare call reaches a function of an #include'd file
// vs t6: same
// expect: f0 T0 lib_only=7

#include zz_harness\zz_bare_lib;

main()
{
	a = lib_only();
	logprint("lib_only=" + a);
}
