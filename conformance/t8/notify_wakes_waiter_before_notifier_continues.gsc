// question: does a woken waiter run inside the notify, before the notifier's next statement?
// fact: notify does not run the waiter inside the call: the notifier runs on to its next wait or its end, then the woken threads run in the same frame
// vs t7: same
// evidence: static: the Notify handler (0x2752160) calls VM_Notify (0x2774090), which kills the endon'd threads and runs endon callbacks at once (AllocThread 0x2758ab0 + VM_ExecuteInternal 0x276efc0), then for each waiter of the event writes its result struct (0x27747c0) and queues it in the frame queue for the current frame (0x2774ca0); a waiter never runs inside the notify
// expect: f0 T0 notifying
// expect: f0 T0 after notify
// expect: f0 T1 woke
// expect: f0 T1 woke still running
#namespace zz_test;

function main()
{
	thread waiter();
	iprintln("notifying");
	level notify(#"go");
	iprintln("after notify");
}

function waiter()
{
	level waittill(#"go");
	iprintln("woke");
	iprintln("woke still running");
}
