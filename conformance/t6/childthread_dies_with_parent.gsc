// question: does a childthread stop when the thread that started it ends?
// vs t7: same: not a keyword (the official T7 compiler's error)
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: childthread does not exist in T6; the compiler (gsc-tool) refuses the script
// expect compile error: local variable 'childthread' not found

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
