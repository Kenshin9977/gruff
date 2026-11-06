// question: reading a field nobody set: undefined, no error?
// fact: reading an unset field gives undefined, no error
// vs t8: same
// expect: f0 T0 defined=0
#namespace zz_test;

function main()
{
	s = spawnstruct();
	iprintln("defined=" + isdefined(s.nothing));
}
