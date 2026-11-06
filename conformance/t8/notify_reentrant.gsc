// question: a woken waiter that notifies a second event: does the second waiter run before the first notifier resumes?
// fact: a notify sent by a woken waiter is deferred the same way
// vs t7: same
// evidence: static: the Notify handler (0x2752160) calls VM_Notify (0x2774090), which kills the endon'd threads and runs endon callbacks at once (AllocThread 0x2758ab0 + VM_ExecuteInternal 0x276efc0), then for each waiter of the event writes its result struct (0x27747c0) and queues it in the frame queue for the current frame (0x2774ca0); a waiter never runs inside the notify
// expect: f0 T0 notify a
// expect: f0 T0 after notify a
// expect: f0 T1 got a
// expect: f0 T1 after notify b
// expect: f0 T2 got b
#namespace zz_test;

function main()
{
	thread first();
	thread second();
	iprintln("notify a");
	level notify(#"a");
	iprintln("after notify a");
}

function first()
{
	level waittill(#"a");
	iprintln("got a");
	level notify(#"b");
	iprintln("after notify b");
}

function second()
{
	level waittill(#"b");
	iprintln("got b");
}
