// question: two endons on one thread: does either event kill it?
// expect: f0 T1 a waiting
// expect: f0 T2 b waiting
// expect: f1 T0 sent second
// fact: each of several endons kills the thread
// vs iw5: same

main()
{
	thread waiter("a");
	thread waiter("b");
	wait 0.05;
	level notify("second");
	logprint("sent second");
	wait 0.05;
	level notify("go");
}

waiter(name)
{
	level endon("first");
	level endon("second");
	logprint(name + " waiting");
	level waittill("go");
	logprint(name + " woke");
}
