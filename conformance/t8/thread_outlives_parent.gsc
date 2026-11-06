// question: a plain thread keeps running after the thread that started it ends (control for childthread)
// fact: a plain thread outlives the thread that started it; in the frame where both resume, the child, which started waiting first, runs first
// vs t7: T7 resumes the parent first (its same-time waits are last-in-first-out)
// evidence: static: the scheduler is a set of sorted execution queues (scrVmPub+0x18 time, +0x2020 frame, +0x4028 timeouts), not T7's per-time variable lists; VM_SetTime (0x2773570) and Scr_RunCurrentThreads (0x2775700) take the due entries from the head
// expect: f0 T1 parent start
// expect: f0 T2 tick 0
// expect: f1 T2 tick 1
// expect: f1 T1 parent end
// expect: f2 T2 tick 2
// expect: f3 T2 tick 3
#namespace zz_test;

function main()
{
	thread parent();
}

function parent()
{
	iprintln("parent start");
	thread child();
	wait 0.05;
	iprintln("parent end");
}

function child()
{
	for (i = 0; i < 4; i++)
	{
		iprintln("tick " + i);
		wait 0.05;
	}
}
