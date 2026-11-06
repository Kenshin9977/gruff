// question: a bare call to a name that is both a builtin (spawnstruct) and a function of the current namespace: which one runs?
// fact: a bare call resolves to a function of the current namespace before a builtin of the same name; sys:: reaches the builtin
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 bare=42
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 qualified=42
// expect: f0 T0 sys is struct=1
// claim: bo3-gsc-language-doc (agrees): an unqualified call resolves in the current namespace first, then falls back to sys
// claim: gscode-dialect-facts (disagrees): a bare call resolves to the builtin first; threaded calls and & references mean the script function
#namespace zz_test;

function spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

function main()
{
	a = spawnstruct();
	logprint("bare=" + a);
	b = zz_test::spawnstruct();
	logprint("qualified=" + b);
	c = sys::spawnstruct();
	logprint("sys is struct=" + (isdefined(c) && c !== 42));
}
