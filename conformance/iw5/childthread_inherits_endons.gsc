// question: a childthread started after `level endon("stop")`: is it killed by that event, while a plain thread started the same way is not?
// fact: a childthread inherits the endons active in the thread that starts it: the starter's endon event kills the child too, while a plain thread started at the same point keeps running
// evidence: static: 0x29aa20 (the child-thread call's thread allocation) copies the endons of the calling thread and its parents onto the new thread (0x2a2540)
// vs iw3: IW3 has no childthread (its compiler reads it as an uninitialised variable)
// expect: f0 T2 child tick 0
// expect: f0 T3 plain tick 0
// expect: f1 T0 sent stop
// expect: f1 T3 plain tick 1
// expect: f2 T3 plain tick 2

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
