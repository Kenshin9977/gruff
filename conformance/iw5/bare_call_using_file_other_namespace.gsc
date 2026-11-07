// question: a bare call to a function that only an #include'd file defines: is it found?
// with: zz_lib_only.gsc
// fact: yes: the compiler emits a far call (file and function by name, opcode 0x29) and the game's loader links it to the #include'd file's function (loading that file through the same asset lookup)
// evidence: static: the loader (0x29f850) records each far call by file and function (0x295fa0); Scr_LoadScript's link step (0x295dc0) patches the call with the function's code position or fails with 'unknown function'; the compiler is gsc-tool
// vs iw3: same
// expect: f0 T0 lib_only=7
#include zz\zz_lib_only;

main()
{
	a = lib_only();
	logprint("lib_only=" + a);
}
