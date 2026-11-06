// question: an array stored in a struct field, then read into a local and changed: does the field change?
// fact: a local read from a struct field, then written: the field keeps its array (the write copies)
// vs t7: a local read from a struct field shares the field's array
// evidence: static: the array-reference path 0x7e9640 (used for every write through an array): when the array object's reference count (u16 at +0 of its object entry) is not 0, RemoveRefToObject (0x7e7cb0), Scr_AllocArray (0x7e58d0) and CopyArray (0x7e5e30) give the variable its own copy (0x7e9882-0x7e98a4): copy on write
// expect: f0 T0 field[0]=1

main()
{
	s = spawnstruct();
	s.arr = [];
	s.arr[0] = 1;
	local = s.arr;
	local[0] = 2;
	logprint("field[0]=" + s.arr[0]);
}
