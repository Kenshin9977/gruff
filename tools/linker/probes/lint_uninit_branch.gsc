// Probe lint-uninit-branch: a local assigned in one branch only, then read.
#namespace zz_probe;

function autoexec main()
{
	a = 1;
	if ( a )
	{
		y = 1;
	}
	x = y;
}
