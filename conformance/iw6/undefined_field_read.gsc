// question: reading a field nobody set: undefined, no error?
// expect: f0 T0 defined=0
// fact: reading a field never set gives undefined, no error
// vs iw5: same

main()
{
	s = spawnstruct();
	logprint("defined=" + isdefined(s.nothing));
}
