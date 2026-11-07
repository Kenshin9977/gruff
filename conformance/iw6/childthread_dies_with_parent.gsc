// question: does a childthread stop when the thread that started it ends?
// fact: childthread is an IW6 form (the child-thread call opcodes), but the child does not stop when the thread that started it ends: it only inherits the starter's endons
// vs iw5: same
// expect: f0 T1 parent start
// expect: f0 T2 child tick 0
// expect: f1 T2 child tick 1
// expect: f2 T2 child tick 2
// expect: f2 T1 parent end
// expect: f3 T2 child tick 3
// expect: f4 T2 child tick 4
// expect: f5 T2 child tick 5
// expect: f6 T2 child tick 6
// expect: f7 T2 child tick 7
// expect: f8 T2 child tick 8
// expect: f9 T2 child tick 9

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
