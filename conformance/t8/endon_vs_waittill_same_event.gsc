// question: a thread that both endon()s and waittill()s the same event: woken or killed?
// fact: endon and waittill on the same event: killed, not woken (VM_Notify handles endons before waiters)
// vs t7: same
// evidence: static: the Notify handler (0x2752160) calls VM_Notify (0x2774090), which kills the endon'd threads and runs endon callbacks at once (AllocThread 0x2758ab0 + VM_ExecuteInternal 0x276efc0), then for each waiter of the event writes its result struct (0x27747c0) and queues it in the frame queue for the current frame (0x2774ca0); a waiter never runs inside the notify
// expect: f0 T1 waiting
// expect: f1 T0 sent
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e");
	iprintln("sent");
}

function waiter()
{
	level endon(#"e");
	iprintln("waiting");
	level waittill(#"e");
	iprintln("woke");
}
