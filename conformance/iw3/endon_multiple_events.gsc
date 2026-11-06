// question: endon with two events: does the second one kill the thread?
// unsupported: IW3 endon takes one event: the game's compiler refuses `endon("first", "second")` (bad syntax)
// expect compile error: bad syntax
// vs t4: same (the form cannot exist in T4 either)

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
