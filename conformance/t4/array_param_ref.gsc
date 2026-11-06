// question: an array passed to a parameter declared &arr and changed in the callee: does the caller see the change?
// unsupported: T4 has no reference parameters: the game's compiler rejects `&arr` in a parameter list (bad syntax)
// expect compile error: bad syntax
// vs t7: T7 shares the array with a &name parameter

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
	iprintln("a[0]=" + a[0] + " a.size=" + a.size);
}
