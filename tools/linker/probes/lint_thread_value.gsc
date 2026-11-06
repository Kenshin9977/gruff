// Probe lint-thread-value: the value of a thread call assigned.
#namespace zz_probe;

function f()
{
	return 1;
}

function autoexec main()
{
	x = thread f();
}
