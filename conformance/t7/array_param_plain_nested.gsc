// question: the copy made for a plain array parameter: shallow (inner arrays shared) or deep?
// fact: the parameter copy is deep: a write into an inner array through the copy is not seen by the caller
// expect: f0 T0 inner=1 outer.size=1
// evidence: static: CopyArray (0x12da210) allocates a new array and calls itself for every child of type array (0x19): a deep copy
#namespace zz_test;

function change(arr)
{
	arr[0][0] = 2;
	arr[1] = "new";
}

function main()
{
	a = [];
	a[0] = [];
	a[0][0] = 1;
	change(a);
	logprint("inner=" + a[0][0] + " outer.size=" + a.size);
}
