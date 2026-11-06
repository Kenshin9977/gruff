// question: a non-array value passed to a &name parameter
// vs t7: a non-array to &name is the script error 'Var ByRef only supported for array types', then a broken error recovery
// evidence: static: SafeCreateLocalVariables (opcode 0x17, handler 0x7edc7f) reads a count and one aligned u16 name per parameter (no flag byte) and moves the argument value into the local as is
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: T6 has no &name parameters (see array_param_ref); the compiler (gsc-tool) refuses the script
// expect compile error: expected identifier, got '&'

change(&v)
{
}

main()
{
	x = 5;
	change(x);
	logprint("x=" + x);
}
