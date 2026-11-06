// question: a childthread started after `level endon("stop")`: is it killed by that event, while a plain thread started the same way is not?
// unsupported: IW3 has no childthread: the game's compiler reads `childthread` as an uninitialised variable and refuses the script
// expect compile error: uninitialised variable 'childthread'
// vs t4: same (the form cannot exist in T4 either)

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
