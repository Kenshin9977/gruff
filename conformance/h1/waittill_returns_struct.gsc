// question: what does waittill return: a struct holding the event and the payload?
// unsupported: H1 waittill is a statement with no value: the compiler (gsc-tool) refuses `s = level waittill("e")`
// expect compile error: expected ';', got 'waittill'
// evidence: the compiler is gsc-tool (H1 ships no compiler: its scripts are bytecode compiled offline)
// vs s1: same

main()
{
	thread waiter();
	wait 0.05;
	level notify("e", 1);
	logprint("sent");
}

waiter()
{
	s = level waittill("e");
	logprint("woke");
}
