// Probe lint-case-variable: a non-constant case label.
#namespace zz_probe;

function autoexec main()
{
	x = 1;
	y = 2;
	switch ( x )
	{
		case y:
			break;
	}
}
