// Probe t7-bare-call: bare calls to a builtin function and a builtin method with no script
// function of that name, plus a bare call to a name that exists nowhere.
#namespace zz_probe;

function autoexec main()
{
	a = spawnstruct();
	b = self getentitynumber();
	zz_no_such_function_anywhere();
}
