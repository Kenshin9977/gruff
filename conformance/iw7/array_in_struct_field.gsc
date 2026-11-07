// question: an array stored in a struct field, then read into a local and changed: does the field change?
// fact: a local read from a struct field and written does not change the field: the write copies the shared array
// expect: f0 T0 field[0]=1
// vs s1: same

main()
{
	s = spawnstruct();
	s.arr = [];
	s.arr[0] = 1;
	local = s.arr;
	local[0] = 2;
	logprint("field[0]=" + s.arr[0]);
}
