// question: an array passed to a plain parameter and changed in the callee: does the caller see the change?
// fact: an array passed to a parameter and written in the callee: the caller does not see the change (the parameter shares the array until the write copies it)
// vs t7: same answer; T7 copies at the call (SafeCreateLocalVariables), T6 at the first write
// evidence: static: SafeCreateLocalVariables (opcode 0x17, handler 0x7edc7f) reads a count and one aligned u16 name per parameter (no flag byte) and moves the argument value into the local as is
// evidence: static: the array-reference path 0x7e9640 (used for every write through an array): when the array object's reference count (u16 at +0 of its object entry) is not 0, RemoveRefToObject (0x7e7cb0), Scr_AllocArray (0x7e58d0) and CopyArray (0x7e5e30) give the variable its own copy (0x7e9882-0x7e98a4): copy on write
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
