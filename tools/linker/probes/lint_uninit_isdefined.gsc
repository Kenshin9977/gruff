// Probe lint-uninit-isdefined: isdefined() of a local that nothing assigns.
#namespace zz_probe;

function autoexec main()
{
	if ( isdefined( y ) )
	{
		x = 1;
	}
}
