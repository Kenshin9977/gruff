// Probe lint-uninit-local: a local read that nothing assigns.
#namespace zz_probe;

function autoexec main()
{
	x = y + 1;
}
