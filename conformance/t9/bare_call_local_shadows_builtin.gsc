// question: a bare call to a name that is both a builtin (spawnstruct) and a function of the current namespace: which one runs?
// fact: a bare call reaches the current namespace's function before a builtin of the same name; `sys::` reaches the builtin
// vs t8: same
// claim: acts-t8-link (agrees): an import ns::f is looked up in the script's exports first; a get-call falls back to the builtin
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 bare=42
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 qualified=42
// expect: f0 T0 sys is struct=1
#namespace zz_test;

function spawnstruct()
{
	iprintln("script spawnstruct called");
	return 42;
}

function main()
{
	a = spawnstruct();
	iprintln("bare=" + a);
	b = zz_test::spawnstruct();
	iprintln("qualified=" + b);
	c = sys::spawnstruct();
	iprintln("sys is struct=" + (isdefined(c) && c !== 42));
}
