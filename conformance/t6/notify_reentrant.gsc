// question: a woken waiter that notifies a second event: does the second waiter run before the first notifier resumes?
// fact: a notify sent by a woken waiter is deferred the same way: each woken thread runs after the current one yields
// vs t7: same
// evidence: static: VM_Execute (0x7ed7e0) is entered only from VM_Resume (0x7f15c0, called only by VM_SetTime 0x7f1b60) and the three Scr_Exec*Thread (0x7f1970, 0x7f1a20, 0x7f1ad0): no notify path runs a waiter inside the call
// expect: f0 T0 notify a
// expect: f0 T0 after notify a
// expect: f0 T1 got a
// expect: f0 T1 after notify b
// expect: f0 T2 got b

main()
{
	thread first();
	thread second();
	logprint("notify a");
	level notify("a");
	logprint("after notify a");
}

first()
{
	level waittill("a");
	logprint("got a");
	level notify("b");
	logprint("after notify b");
}

second()
{
	level waittill("b");
	logprint("got b");
}
