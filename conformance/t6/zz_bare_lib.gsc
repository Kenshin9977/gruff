// helper (not a test): a function named like a builtin, and one only this namespace defines
// helper: yes

spawnstruct()
{
	logprint("lib spawnstruct called");
	return 42;
}

lib_only()
{
	return 7;
}
