// question: an array passed to a plain parameter and changed in the callee: does the caller see the change?
// fact: an array passed to a parameter and written in the callee: the caller does not see the change (the parameter shares the array until the write copies it)
// vs t6: same
// expect: f0 T0 a[0]=1 a.size=1

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
