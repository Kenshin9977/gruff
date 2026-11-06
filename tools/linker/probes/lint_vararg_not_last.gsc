// Probe lint-vararg-not-last: a parameter after the vararg.
#namespace zz_probe;

function f( ..., a )
{
	return a;
}

function autoexec main()
{
	f( 1, 2 );
}
