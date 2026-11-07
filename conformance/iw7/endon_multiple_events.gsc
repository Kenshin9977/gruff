// question: endon with two events: does the second one kill the thread?
// unsupported: IW7 endon takes one event: the compiler (gsc-tool) refuses `endon("first", "second")`
// expect compile error: expected ')', got ','
// evidence: the compiler is gsc-tool (IW7 ships no compiler: its scripts are bytecode compiled offline)
// vs s1: same

main()
{
	thread waiter();
	wait 0.05;
	level notify("second");
	logprint("sent second");
	wait 0.05;
	level notify("go");
}

waiter()
{
	level endon("first", "second");
	logprint("waiting");
	level waittill("go");
	logprint("woke (should not happen)");
}
