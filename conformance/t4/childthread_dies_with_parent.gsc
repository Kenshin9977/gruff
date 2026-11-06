// question: does a childthread stop when the thread that started it ends?
// unsupported: T4 has no childthread: the game's compiler reads `childthread` as an uninitialised local variable and refuses the script
// expect compile error: uninitialised variable 'childthread'
// vs t7: same (T7's linker: Uninitialized local variable 'childthread')

main()
{
	thread parent();
}

parent()
{
	iprintln("parent start");
	childthread child();
	wait 0.1;
	iprintln("parent end");
}

child()
{
	for (i = 0; i < 10; i++)
	{
		iprintln("child tick " + i);
		wait 0.05;
	}
}
