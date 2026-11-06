// question: in which order do two waiters on the same event wake?
// fact: the last registered waiter wakes first, both after the notifier yields
// vs t7: same
// evidence: static: the Notify handler (0x2752160) calls VM_Notify (0x2774090), which kills the endon'd threads and runs endon callbacks at once (AllocThread 0x2758ab0 + VM_ExecuteInternal 0x276efc0), then for each waiter of the event writes its result struct (0x27747c0) and queues it in the frame queue for the current frame (0x2774ca0); a waiter never runs inside the notify
// expect: f1 T0 notifying
// expect: f1 T0 after notify
// expect: f1 T2 second
// expect: f1 T1 first
#namespace zz_test;

function main()
{
	thread waiter("first");
	thread waiter("second");
	wait 0.05;
	iprintln("notifying");
	level notify(#"go");
	iprintln("after notify");
}

function waiter(name)
{
	level waittill(#"go");
	iprintln(name);
}
