// question: a childthread started after `level endon("stop")`: is it killed by that event, while a plain thread started the same way is not?
// unsupported: T4 has no childthread: the game's compiler reads `childthread` as an uninitialised variable and refuses the script
// expect compile error: uninitialised variable 'childthread'
// vs t7: T7 has no childthread either (T8 copies the starter's endons onto it)

main()
{
	thread parent();
	wait 0.05;
	level notify("stop");
	iprintln("sent stop");
	wait 0.1;
}

parent()
{
	level endon("stop");
	childthread ticker("child");
	thread ticker("plain");
	wait 1;
	iprintln("parent end (should not happen)");
}

ticker(name)
{
	for (i = 0; i < 3; i++)
	{
		iprintln(name + " tick " + i);
		wait 0.05;
	}
}
