// Probe t7-array-passing: the same body with a plain and an &-declared parameter,
// and the same array argument passed to both.
#namespace zz_probe;

function by_plain(arr)
{
	arr[0] = 1;
	arr = undefined;
}

function by_amp(&arr)
{
	arr[0] = 1;
	arr = undefined;
}

function autoexec main()
{
	a = [];
	by_plain(a);
	by_amp(a);
	thread by_amp(a);
	self by_amp(a);
	by_plain(level.zz_arr);
	by_amp(level.zz_arr);
}
