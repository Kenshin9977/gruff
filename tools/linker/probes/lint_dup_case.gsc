// Probe lint-dup-case: two case labels with the same value in one switch.
#namespace zz_probe;

function autoexec main()
{
	x = 1;
	switch ( x )
	{
		case 1:
			break;
		case 1:
			break;
	}
}
