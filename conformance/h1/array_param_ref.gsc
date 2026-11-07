// question: an array passed to a parameter declared &arr and changed in the callee: does the caller see the change?
// unsupported: H1 has no reference parameters: the compiler (gsc-tool) rejects `&arr` in a parameter list
// expect compile error: expected identifier, got '&'
// evidence: the compiler is gsc-tool (H1 ships no compiler: its scripts are bytecode compiled offline)
// vs s1: same

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
