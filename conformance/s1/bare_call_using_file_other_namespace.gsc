// question: a bare call to a function that only an #include'd file defines: is it found?
// with: zz_lib_only.gsc
// fact: yes: the compiler emits a far call (file and function by name) and the game's loader links it to the #include'd file's function (loading that file through the same asset lookup)
// vs iw6: same
// expect: f0 T0 lib_only=7
#include zz\zz_lib_only;

main()
{
	a = lib_only();
	logprint("lib_only=" + a);
}
