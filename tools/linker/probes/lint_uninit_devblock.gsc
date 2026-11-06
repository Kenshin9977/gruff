// Probe lint-uninit-devblock: a local assigned only in a dev block, read outside it.
#namespace zz_probe;

function autoexec main()
{
	/#
	y = 1;
	#/
	x = y;
}
