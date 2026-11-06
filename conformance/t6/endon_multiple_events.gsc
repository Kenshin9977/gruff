// question: endon with two events: does the second one kill the thread?
// vs t7: same: T7 endon takes one event
// evidence: static: EndOn (0x7ef62c): object at the top, a string at top-1, nothing else
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: endon takes one event in T6; the compiler (gsc-tool) refuses the script
// expect compile error: expected ')', got ','

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
