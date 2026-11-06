// Probe lint-assign-arg: an assignment as a call argument.
#namespace zz_probe;

function f( a )
{
	return a;
}

function autoexec main()
{
	x = 1;
	f( x = 2 );
}
