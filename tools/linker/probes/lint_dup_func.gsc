// Probe lint-dup-func: two functions with the same name in one file.
#namespace zz_probe;

function f()
{
}

function f()
{
}

function autoexec main()
{
	f();
}
