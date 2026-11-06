// question: does waittill return a value (s = level waittill(e))?
// vs t6: same
// unsupported: the form does not exist in T5: waittill is a statement; the game's own compiler refuses the script at load
// expect compile error: bad syntax

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
