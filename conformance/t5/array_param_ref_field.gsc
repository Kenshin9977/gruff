// question: an array held in a level field, passed to a &arr parameter and to a plain one: what does level.arr hold afterwards?
// vs t6: same: T6 has none either
// unsupported: the form does not exist in T5: T5 has no &name parameters; the game's own compiler refuses the script at load
// expect compile error: bad syntax

plain(arr)
{
	arr[0] = 50;
}

by_ref(&arr)
{
	arr[0] = 99;
}

main()
{
	level.arr = [];
	level.arr[0] = 1;
	plain(level.arr);
	logprint("after plain: " + level.arr[0]);
	by_ref(level.arr);
	logprint("after ref: " + level.arr[0]);
}
