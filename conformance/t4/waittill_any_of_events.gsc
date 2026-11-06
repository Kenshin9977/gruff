// question: waittill with two events wakes on either?
// unsupported: T4 waittill takes one event, then the names to bind: a second event string is not a name (bad syntax)
// expect compile error: bad syntax
// vs t7: T7 waittill takes one event too (T8: several events)

main()
{
	thread waiter();
	wait 0.05;
	level notify("b");
	iprintln("sent b");
	wait 0.05;
	level notify("a");
	iprintln("sent a");
}

waiter()
{
	level waittill("a", "b");
	iprintln("woke");
}
