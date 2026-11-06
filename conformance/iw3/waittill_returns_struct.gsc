// question: what does waittill return: a struct holding the event and the payload?
// unsupported: IW3 waittill is a statement with no value: the game's compiler refuses `s = level waittill("e")` (bad syntax)
// expect compile error: bad syntax
// vs t4: same (the form cannot exist in T4 either)

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
