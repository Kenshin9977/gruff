// question: x = thread f(): accepted, and what does x hold when f returns before or after its first wait?
// fact: the compiler accepts x = thread f(); x is f's return value when f ends without waiting, undefined when f waits first
// expect: f0 T0 x defined=0
// expect: f0 T0 y=6
// vs s1: same

five()
{
	wait 0.05;
	return 5;
}

six()
{
	return 6;
}

main()
{
	x = thread five();
	logprint("x defined=" + isdefined(x));
	y = thread six();
	logprint("y=" + y);
}
