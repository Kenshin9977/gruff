// question: an array passed to a parameter declared &arr and changed in the callee: does the caller see the change?
// vs t6: same: T6 has none either
// unsupported: the form does not exist in T5: T5 has no &name parameters; the game's own compiler refuses the script at load
// expect compile error: bad syntax

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
