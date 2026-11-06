// Probe t6-foreach (cross-check on the T7 compiler): the hidden locals a foreach creates.
#namespace zz_probe;

function autoexec main()
{
	arr = [];
	foreach ( v in arr )
	{
		x = v;
	}
	foreach ( k, v2 in arr )
	{
		y = k;
	}
}
