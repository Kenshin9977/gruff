// question: can waittill take two events?
// vs t7: same: T7 waittill takes one event
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: no; the compiler (gsc-tool) refuses the script
// expect compile error: illegal waittill param, must be a local variable or undefined

main()
{
	thread waiter();
	wait 0.05;
	level notify("b");
	logprint("sent b");
	wait 0.05;
	level notify("a");
	logprint("sent a");
}

waiter()
{
	level waittill("a", "b");
	logprint("woke");
}
