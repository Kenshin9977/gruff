// Probe lint-unreachable: a statement after return.
#namespace zz_probe;

function f()
{
	return 1;
	x = 2;
}

function autoexec main()
{
	f();
}
