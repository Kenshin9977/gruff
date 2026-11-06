// question: waittill with two events wakes on either?
// unsupported: IW3 waittill takes one event, then the names to bind: a second event string is not a name (bad syntax)
// expect compile error: bad syntax
// vs t4: same (the form cannot exist in T4 either)

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
