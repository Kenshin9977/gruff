// question: an array stored in a struct field, then read into a local and changed: does the field change?
// fact: a local read from a struct field and written does not change the field: the write copies the shared array
// expect: f0 T0 field[0]=1
// evidence: static: copy on write in the array-ref evaluator (0x516ff0, the reference-count test at 0x516fb5), as array_copy_on_assign
// vs t4: same

main()
{
	s = spawnstruct();
	s.arr = [];
	s.arr[0] = 1;
	local = s.arr;
	local[0] = 2;
	logprint("field[0]=" + s.arr[0]);
}
