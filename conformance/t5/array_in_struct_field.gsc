// question: an array stored in a struct field, then read into a local and changed: does the field change?
// fact: a local read from a struct field, then written: the field keeps its array (the write copies)
// vs t6: same
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
