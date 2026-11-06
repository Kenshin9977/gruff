// question: an endon fired while the thread waits on another event: is the thread killed?
// fact: an endon notify kills a thread blocked in waittill on another event
// vs t6: same
// expect: f0 T1 waiting
// expect: f1 T0 sent stop
// expect: f1 T0 sent go

main()
{
	thread waiter();
	wait 0.05;
	level notify("stop");
	logprint("sent stop");
	level notify("go");
	logprint("sent go");
}

waiter()
{
	level endon("stop");
	logprint("waiting");
	level waittill("go");
	logprint("woke (should not happen)");
}
