// question: an endon notify while the thread is in a timed wait: killed before the wait ends?
// fact: an endon kills a thread in a timed wait
// vs t7: same
// evidence: static: the Notify handler (0x2752160) calls VM_Notify (0x2774090), which kills the endon'd threads and runs endon callbacks at once (AllocThread 0x2758ab0 + VM_ExecuteInternal 0x276efc0), then for each waiter of the event writes its result struct (0x27747c0) and queues it in the frame queue for the current frame (0x2774ca0); a waiter never runs inside the notify
// expect: f0 T1 waiting
// expect: f1 T0 sent stop
#namespace zz_test;

function main()
{
	thread sleeper();
	wait 0.05;
	level notify(#"stop");
	iprintln("sent stop");
	wait 0.2;
}

function sleeper()
{
	level endon(#"stop");
	iprintln("waiting");
	wait 0.1;
	iprintln("woke (should not happen)");
}
