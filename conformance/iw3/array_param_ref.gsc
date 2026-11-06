// question: an array passed to a parameter declared &arr and changed in the callee: does the caller see the change?
// unsupported: IW3 has no reference parameters: the game's compiler rejects `&arr` in a parameter list (bad syntax)
// expect compile error: bad syntax
// vs t4: same (the form cannot exist in T4 either)

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
