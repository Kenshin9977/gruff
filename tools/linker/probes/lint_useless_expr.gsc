// Probe lint-useless-expr: an expression statement with no effect.
#namespace zz_probe;

function autoexec main()
{
	a = 1;
	b = 2;
	a + b;
}
