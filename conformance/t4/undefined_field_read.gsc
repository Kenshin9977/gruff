// question: reading a field nobody set: undefined, no error?
// expect: f0 T0 defined=0
// fact: reading a field never set gives undefined, no error
// vs t7: same

main()
{
	s = spawnstruct();
	iprintln("defined=" + isdefined(s.nothing));
}
