// question: an array passed to a plain parameter and changed in the callee: does the caller see the change?
// expect: f0 T0 a[0]=1 a.size=1
// fact: an array passed to a plain parameter and written in the callee is not changed for the caller (the callee's write copies it)
// evidence: static: no copy at the call (T4 has no reference parameters); the callee's first write copies the shared array (0x692850, 0x692ab6)
// vs t7: same (T7 copies on entry, in SafeCreateLocalVariables)
// claim: gscode-dialect-facts (agrees): array_argument_passing = copy (profiles/t4.toml)

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
	iprintln("a[0]=" + a[0] + " a.size=" + a.size);
}
