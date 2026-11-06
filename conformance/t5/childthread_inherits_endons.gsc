// question: a childthread started after `level endon("stop")`: is it killed by that event?
// vs t6: same: not a keyword in T6 either
// unsupported: the form does not exist in T5: childthread does not exist in T5; the game's own compiler refuses the script at load
// expect compile error: uninitialised variable 'childthread'

main()
{
	thread parent();
	wait 0.05;
	level notify("stop");
	logprint("sent stop");
	wait 0.1;
}

parent()
{
	level endon("stop");
	childthread ticker("child");
	thread ticker("plain");
	wait 1;
	logprint("parent end (should not happen)");
}

ticker(name)
{
	for (i = 0; i < 3; i++)
	{
		logprint(name + " tick " + i);
		wait 0.05;
	}
}
