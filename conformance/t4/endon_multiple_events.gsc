// question: endon with two events: does the second one kill the thread?
// unsupported: T4 endon takes one event: the game's compiler refuses `endon("first", "second")` (bad syntax)
// expect compile error: bad syntax
// vs t7: T7 endon takes one event too (T8: either event kills)

main()
{
	thread waiter();
	wait 0.05;
	level notify("second");
	iprintln("sent second");
	wait 0.05;
	level notify("go");
}

waiter()
{
	level endon("first", "second");
	iprintln("waiting");
	level waittill("go");
	iprintln("woke (should not happen)");
}
