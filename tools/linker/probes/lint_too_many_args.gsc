// Probe lint-too-many-args: a script function called with more arguments than it declares.
#namespace zz_probe;

function g( a )
{
	return a;
}

function autoexec main()
{
	g( 1, 2 );
}
