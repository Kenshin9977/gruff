// question: a non-array value passed to a &name parameter
// unsupported: IW6 has no reference parameters: the compiler (gsc-tool) rejects `&v` in a parameter list
// expect compile error: expected identifier, got '&'
// evidence: the compiler is gsc-tool (IW6 ships no compiler: its scripts are bytecode compiled offline)
// vs iw5: same

change(&v)
{
}

main()
{
	x = 5;
	change(x);
	logprint("x=" + x);
}
