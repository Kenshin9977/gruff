// question: an array held in a level field, passed to a plain parameter and changed in the callee: what does level.arr hold afterwards?
// fact: level.arr passed to a plain parameter is not changed by the callee (IW6 has no &name parameter: the T7 test's second half cannot exist)
// expect: f0 T0 after plain: 1
// vs iw5: same

plain(arr)
{
	arr[0] = 50;
}

main()
{
	level.arr = [];
	level.arr[0] = 1;
	plain(level.arr);
	logprint("after plain: " + level.arr[0]);
}
