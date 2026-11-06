// Probe t7-bare-call: a script function named like a builtin method, called as a method.
#namespace zz_probe;

function getentitynumber()
{
	return 7;
}

function autoexec main()
{
	a = self getentitynumber();
	self thread getentitynumber();
}
