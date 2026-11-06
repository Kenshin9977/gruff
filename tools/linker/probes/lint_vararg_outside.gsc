// Probe lint-vararg-outside: vararg read in a function without ....
#namespace zz_probe;

function autoexec main()
{
	x = vararg;
}
