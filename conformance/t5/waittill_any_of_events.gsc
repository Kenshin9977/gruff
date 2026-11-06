// question: can waittill take two events?
// vs t6: same
// unsupported: the form does not exist in T5: waittill takes one event, then names; the game's own compiler refuses the script at load
// expect compile error: bad syntax

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
