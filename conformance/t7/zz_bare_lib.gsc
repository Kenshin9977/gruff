// helper (not a test): a function named like a builtin, and one only this namespace defines
// helper: yes
#namespace zz_bare_lib;

function spawnstruct()
{
	logprint("lib spawnstruct called");
	return 42;
}

function lib_only()
{
	return 7;
}
