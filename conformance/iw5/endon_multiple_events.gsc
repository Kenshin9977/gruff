// question: endon with two events: does the second one kill the thread?
// unsupported: IW5 endon takes one event: the compiler (gsc-tool) refuses `endon("first", "second")`
// expect compile error: expected ')', got ','
// evidence: the compiler is gsc-tool (IW5 ships no compiler: its scripts are bytecode compiled offline)
// vs iw3: same (the form cannot exist in IW3 either)

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
