// question: reading a field nobody set: undefined, no error?
// fact: reading a field never set gives undefined, no error
// vs t7: same
// expect: f0 T0 defined=0

main()
{
	s = spawnstruct();
	logprint("defined=" + isdefined(s.nothing));
}
