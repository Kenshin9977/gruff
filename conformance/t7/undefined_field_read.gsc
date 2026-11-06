// question: reading a field nobody set: undefined, no error?
// expect: f0 T0 defined=0
// fact: reading a field never set gives undefined, no error
#namespace zz_test;

function main()
{
	s = spawnstruct();
	logprint("defined=" + isdefined(s.nothing));
}
