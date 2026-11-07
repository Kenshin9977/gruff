// question: waittill with two events wakes on either?
// unsupported: S1 waittill takes one event, then the names to bind: the compiler (gsc-tool) refuses a second event string
// expect compile error: expected identifier or 'undefined' in waittill arguments
// evidence: the compiler is gsc-tool (S1 ships no compiler: its scripts are bytecode compiled offline)
// vs iw6: same

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
