// question: the copy made for a plain array parameter: shallow (inner arrays shared) or deep?
// fact: a write into an inner array through a parameter is not seen by the caller: the inner array is shared by the copy and copied in its turn when written
// expect: f0 T0 inner=1 outer.size=1
// vs iw3: same

change(arr)
{
	arr[0][0] = 2;
	arr[1] = "new";
}

main()
{
	a = [];
	a[0] = [];
	a[0][0] = 1;
	change(a);
	logprint("inner=" + a[0][0] + " outer.size=" + a.size);
}
