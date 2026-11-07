// question: a non-array value passed to a &name parameter
// unsupported: H1 has no reference parameters: the compiler (gsc-tool) rejects `&v` in a parameter list
// expect compile error: expected identifier, got '&'
// evidence: the compiler is gsc-tool (H1 ships no compiler: its scripts are bytecode compiled offline)
// vs s1: same

change(&v)
{
}

main()
{
	x = 5;
	change(x);
	logprint("x=" + x);
}
