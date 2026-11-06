// Probe lint-const-assign: an assignment to a const local.
#namespace zz_probe;

function autoexec main()
{
	const X = 1;
	X = 2;
}
