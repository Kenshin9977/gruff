// Probe lint-dup-param: a parameter name repeated.
#namespace zz_probe;

function f( a, a )
{
	return a;
}

function autoexec main()
{
	f( 1, 2 );
}
