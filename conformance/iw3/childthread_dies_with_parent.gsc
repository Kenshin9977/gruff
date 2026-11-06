// question: does a childthread stop when the thread that started it ends?
// unsupported: IW3 has no childthread: the game's compiler reads `childthread` as an uninitialised local variable and refuses the script
// expect compile error: uninitialised variable 'childthread'
// vs t4: same

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
