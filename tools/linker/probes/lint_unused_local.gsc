// Probe lint-unused-local: a local assigned and never read.
#namespace zz_probe;

function autoexec main()
{
	x = 1;
}
