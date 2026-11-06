// question: what does waittill return: a struct holding the event and the payload?
// unsupported: T4 waittill is a statement with no value: the game's compiler refuses `s = level waittill("e")` (bad syntax)
// expect compile error: bad syntax
// vs t7: T7 waittill returns nothing either and binds names (T8: a struct)

main()
{
	thread waiter();
	wait 0.05;
	level notify("e", 1);
	iprintln("sent");
}

waiter()
{
	s = level waittill("e");
	iprintln("woke");
}
