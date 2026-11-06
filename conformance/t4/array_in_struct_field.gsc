// question: an array stored in a struct field, then read into a local and changed: does the field change?
// fact: a local read from a struct field and written does not change the field: the write copies the shared array
// expect: f0 T0 field[0]=1
// evidence: static: copy on write in the array-ref evaluator (0x692850, the reference-count test at 0x692ab6), as array_copy_on_assign
// vs t7: T7 shares the field's array (field[0]=2)

main()
{
	s = spawnstruct();
	s.arr = [];
	s.arr[0] = 1;
	local = s.arr;
	local[0] = 2;
	iprintln("field[0]=" + s.arr[0]);
}
