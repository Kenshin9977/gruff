// question: a plain thread keeps running after the thread that started it ends (control for childthread)
// fact: a plain thread outlives the thread that started it; same-time waits resume last-registered first
// expect: f0 T1 parent start
// expect: f0 T2 tick 0
// expect: f1 T1 parent end
// expect: f1 T2 tick 1
// expect: f2 T2 tick 2
// expect: f3 T2 tick 3
// vs iw6: same

main()
{
	thread parent();
}

parent()
{
	logprint("parent start");
	thread child();
	wait 0.05;
	logprint("parent end");
}

child()
{
	for (i = 0; i < 4; i++)
	{
		logprint("tick " + i);
		wait 0.05;
	}
}
