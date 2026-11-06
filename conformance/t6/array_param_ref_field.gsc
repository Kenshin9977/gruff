// question: an array held in a level field, passed to a &arr parameter and to a plain one: what does level.arr hold afterwards?
// vs t7: level.arr passed to a plain parameter is copied, to a &name parameter shared
// evidence: static: SafeCreateLocalVariables (opcode 0x17, handler 0x7edc7f) reads a count and one aligned u16 name per parameter (no flag byte) and moves the argument value into the local as is
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: T6 has no &name parameters (see array_param_ref); the compiler (gsc-tool) refuses the script
// expect compile error: expected identifier, got '&'

plain(arr)
{
	arr[0] = 50;
}

by_ref(&arr)
{
	arr[0] = 99;
}

main()
{
	level.arr = [];
	level.arr[0] = 1;
	plain(level.arr);
	logprint("after plain: " + level.arr[0]);
	by_ref(level.arr);
	logprint("after ref: " + level.arr[0]);
}
