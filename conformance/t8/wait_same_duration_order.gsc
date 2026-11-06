// question: two threads that wait the same time: do they resume in the order they started waiting?
// fact: waits ending in the same frame resume in the order of the wait calls (first in, first out)
// vs t7: T7: reverse order of the wait calls
// evidence: static: the scheduler is a set of sorted execution queues (scrVmPub+0x18 time, +0x2020 frame, +0x4028 timeouts), not T7's per-time variable lists; VM_SetTime (0x2773570) and Scr_RunCurrentThreads (0x2775700) take the due entries from the head
// expect: f1 T1 a
// expect: f1 T2 b
// expect: f1 T3 c
#namespace zz_test;

function main()
{
	thread w("a");
	thread w("b");
	thread w("c");
}

function w(name)
{
	wait 0.05;
	iprintln(name);
}
