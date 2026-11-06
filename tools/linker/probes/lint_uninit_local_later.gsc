// Probe lint-uninit-local-later: a local read before its only assignment (in a loop).
#namespace zz_probe;

function autoexec main()
{
	for ( i = 0; i < 3; i++ )
	{
		if ( i > 0 )
		{
			z = y;
		}
		y = i;
	}
}
