// helper (not a test): a function named like a builtin, and one only this file defines
// helper: yes

spawnstruct()
{
	iprintln("lib spawnstruct called");
	return 42;
}

lib_only()
{
	return 7;
}
