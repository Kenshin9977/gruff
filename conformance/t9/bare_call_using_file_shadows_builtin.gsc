// question: a bare call to a name that is a builtin and also a function of a #using'd file in another namespace: which one runs?
// fact: a same-named function in a #using'd file of another namespace does not shadow the builtin
// vs t8: same
// expect: f0 T0 is struct=1
// with: zz_bare_lib.gsc
#using scripts\zz_harness\zz_bare_lib;
#namespace zz_test;

function main()
{
	a = spawnstruct();
	iprintln("is struct=" + (isdefined(a) && a !== 42));
}
