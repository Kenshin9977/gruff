// question: a childthread started after `level endon("stop")`: is it killed by that event?
// vs t7: same: T7 has no childthread either
// evidence: the compiler is gsc-tool (no official BO2 compiler exists); its rejection is the compile side only
// unsupported: the form does not exist in T6: childthread does not exist in T6 (see childthread_dies_with_parent); the compiler (gsc-tool) refuses the script
// expect compile error: local variable 'childthread' not found

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
