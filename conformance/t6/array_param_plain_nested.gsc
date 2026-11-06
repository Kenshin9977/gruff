// question: the copy made for a plain array parameter: shallow (inner arrays shared) or deep?
// fact: a write into an inner array through a parameter is not seen by the caller: each level is copied on write
// vs t7: same answer; T7 makes a deep copy at the call
// evidence: static: the array-reference path 0x7e9640 (used for every write through an array): when the array object's reference count (u16 at +0 of its object entry) is not 0, RemoveRefToObject (0x7e7cb0), Scr_AllocArray (0x7e58d0) and CopyArray (0x7e5e30) give the variable its own copy (0x7e9882-0x7e98a4): copy on write
// expect: f0 T0 inner=1 outer.size=1

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
