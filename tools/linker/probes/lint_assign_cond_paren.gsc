// Probe lint-assign-cond-paren: a parenthesised assignment as an if condition.
#namespace zz_probe;

function autoexec main()
{
	x = 1;
	if ( ( x = 2 ) )
	{
		x = 3;
	}
}
