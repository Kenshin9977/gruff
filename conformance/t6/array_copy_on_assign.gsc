// question: b = a for an array, then b is changed: does a change too (reference) or not (copy)?
// fact: b = a then a write through b: a is unchanged; arrays behave as values (the write copies the shared array)
// vs t7: b = a shares the array: a write through b is seen through a
// evidence: static: the array-reference path 0x7e9640 (used for every write through an array): when the array object's reference count (u16 at +0 of its object entry) is not 0, RemoveRefToObject (0x7e7cb0), Scr_AllocArray (0x7e58d0) and CopyArray (0x7e5e30) give the variable its own copy (0x7e9882-0x7e98a4): copy on write
// expect: f0 T0 a[0]=1 b[0]=2 a.size=1 b.size=2

main()
{
	a = [];
	a[0] = 1;
	b = a;
	b[0] = 2;
	b[1] = 3;
	logprint("a[0]=" + a[0] + " b[0]=" + b[0] + " a.size=" + a.size + " b.size=" + b.size);
}
