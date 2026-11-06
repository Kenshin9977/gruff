// question: an array passed to a parameter declared &arr and changed in the callee: does the caller see the change?
// vs t7: an array passed to a &name parameter is shared with the caller
// evidence: static: SafeCreateLocalVariables (opcode 0x17, handler 0x7edc7f) reads a count and one aligned u16 name per parameter (no flag byte) and moves the argument value into the local as is
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: T6 has no &name parameters; the compiler (gsc-tool) refuses the script
// expect compile error: expected identifier, got '&'

change(&arr)
{
	arr[0] = 99;
	arr[1] = 100;
}

main()
{
	a = [];
	a[0] = 1;
	change(a);
	logprint("a[0]=" + a[0] + " a.size=" + a.size);
}
