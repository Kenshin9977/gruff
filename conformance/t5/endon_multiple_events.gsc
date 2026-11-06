// question: endon with two events: does the second one kill the thread?
// vs t6: same
// unsupported: the form does not exist in T5: endon takes one event; the game's own compiler refuses the script at load
// expect compile error: bad syntax

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
