// Probe t7-bare-call: a script function named like a builtin (spawnstruct), called four ways.
#namespace zz_probe;

function spawnstruct()
{
	return 42;
}

function autoexec main()
{
	a = spawnstruct();
	b = zz_probe::spawnstruct();
	c = sys::spawnstruct();
	thread spawnstruct();
	f = &spawnstruct;
}
