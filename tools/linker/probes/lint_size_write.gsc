// Probe lint-size-write: an assignment to .size.
#namespace zz_probe;

function autoexec main()
{
	a = [];
	a.size = 2;
}
