// question: does a childthread stop when the thread that started it ends?
// vs t6: same: not a keyword in T6 either
// unsupported: the form does not exist in T5: childthread does not exist in T5; the game's own compiler refuses the script at load
// expect compile error: uninitialised variable 'childthread'

main()
{
	thread parent();
}

parent()
{
	logprint("parent start");
	childthread child();
	wait 0.1;
	logprint("parent end");
}

child()
{
	for (i = 0; i < 10; i++)
	{
		logprint("child tick " + i);
		wait 0.05;
	}
}
