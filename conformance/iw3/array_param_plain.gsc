// question: an array passed to a plain parameter and changed in the callee: does the caller see the change?
// expect: f0 T0 a[0]=1 a.size=1
// fact: an array passed to a plain parameter and written in the callee is not changed for the caller (the callee's write copies it)
// claim: gscode-dialect-facts (agrees): array_argument_passing = copy (profiles/iw3.toml)
// evidence: static: no copy at the call (no reference parameters); the callee's first write copies the shared array (0x516ff0, 0x516fb5)
// vs t4: same

change(arr)
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
