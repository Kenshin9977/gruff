// question: a bare call to a name that is a builtin and also a function of a #using'd file in another namespace: which one runs?
// with: zz_bare_lib.gsc
// expect: f0 T0 is struct=1
// fact: a function of the same name in a #using'd file of another namespace does not shadow the builtin for a bare call
#using scripts\zz_harness\zz_bare_lib;
#namespace zz_test;

function main()
{
	a = spawnstruct();
	logprint("is struct=" + (isdefined(a) && a !== 42));
}
