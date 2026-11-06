// question: an array held in a level field, passed to a &arr parameter and to a plain one: what does level.arr hold afterwards?
// expect: f0 T0 after plain: 1
// expect: f0 T0 after ref: 99
// fact: level.arr passed to a plain parameter is copied, to a &name parameter shared
#namespace zz_test;

function plain(arr)
{
	arr[0] = 50;
}

function by_ref(&arr)
{
	arr[0] = 99;
}

function main()
{
	level.arr = [];
	level.arr[0] = 1;
	plain(level.arr);
	logprint("after plain: " + level.arr[0]);
	by_ref(level.arr);
	logprint("after ref: " + level.arr[0]);
}
