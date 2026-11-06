// question: does waittill return a value (s = level waittill(e))?
// vs t7: same: T7 waittill returns nothing
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: no; the compiler (gsc-tool) refuses the script
// expect compile error: expected ';', got 'waittill'

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
	logprint("s defined=" + isdefined(s));
}
