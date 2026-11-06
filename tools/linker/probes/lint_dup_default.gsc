// Probe lint-dup-default: two default labels in one switch.
#namespace zz_probe;

function autoexec main()
{
	x = 1;
	switch ( x )
	{
		default:
			break;
		default:
			break;
	}
}
