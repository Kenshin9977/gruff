// A library with a private helper, a two-parameter function and a waiting one.
#namespace lib;

function private hidden()
{
}

function pair( a, b )
{
	return a + b;
}

function slow()
{
	wait 1;
	return 1;
}

function quick()
{
	return 1;
}
