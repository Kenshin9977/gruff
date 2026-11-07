// question: an endon notify while the thread is in a timed wait: killed before the wait ends?
// fact: an endon kills a thread in a timed wait too
// expect: f0 T1 waiting
// expect: f1 T0 sent stop
// vs iw5: same

main()
{
	thread sleeper();
	wait 0.05;
	level notify("stop");
	logprint("sent stop");
	wait 0.2;
}

sleeper()
{
	level endon("stop");
	logprint("waiting");
	wait 0.1;
	logprint("woke (should not happen)");
}
