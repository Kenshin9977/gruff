// question: three waiters registered in order a, b, c on one event: wake order?
// fact: waiters wake in reverse order of registration (c, b, a)
// vs t7: same
// evidence: static: the Notify handler (0x2752160) calls VM_Notify (0x2774090), which kills the endon'd threads and runs endon callbacks at once (AllocThread 0x2758ab0 + VM_ExecuteInternal 0x276efc0), then for each waiter of the event writes its result struct (0x27747c0) and queues it in the frame queue for the current frame (0x2774ca0); a waiter never runs inside the notify
// expect: f1 T0 notifying
// expect: f1 T3 c
// expect: f1 T2 b
// expect: f1 T1 a
#namespace zz_test;

function main()
{
	thread waiter("a");
	thread waiter("b");
	thread waiter("c");
	wait 0.05;
	iprintln("notifying");
	level notify(#"go");
}

function waiter(name)
{
	level waittill(#"go");
	iprintln(name);
}
