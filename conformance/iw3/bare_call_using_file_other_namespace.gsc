// question: a bare call to a function that only an #include'd file defines: does the game's compiler find it?
// with: zz_bare_lib.gsc
// fact: yes: #include merges the included file's functions into the caller's scope; a bare call reaches them
// expect: f0 T0 lib_only=7
// vs t4: same
#include zz\zz_bare_lib;

main()
{
	a = lib_only();
	logprint("lib_only=" + a);
}
