// Probe t7-array-passing: an &-parameter fed a non-variable argument.
#namespace zz_probe;

function by_amp(&arr)
{
	arr[0] = 1;
}

function autoexec main()
{
	by_amp(array(1, 2));
}
