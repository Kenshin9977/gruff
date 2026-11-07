// question: a non-array value passed to a &name parameter
// unsupported: IW5 has no reference parameters: the compiler (gsc-tool) rejects `&v` in a parameter list
// expect compile error: expected identifier, got '&'
// evidence: the compiler is gsc-tool (IW5 ships no compiler: its scripts are bytecode compiled offline)
// vs iw3: same (the form cannot exist in IW3 either)

change(&v)
{
}

main()
{
	x = 5;
	change(x);
	logprint("x=" + x);
}
