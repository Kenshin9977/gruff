// Probe lint-useless-cmp: a comparison as a statement.
#namespace zz_probe;

function autoexec main()
{
	a = 1;
	a == 2;
}
