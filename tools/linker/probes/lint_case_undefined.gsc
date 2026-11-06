// Probe lint-case-undefined: case undefined in a switch.
#namespace zz_probe;

function autoexec main()
{
	x = 1;
	switch ( x )
	{
		case undefined:
			break;
	}
}
