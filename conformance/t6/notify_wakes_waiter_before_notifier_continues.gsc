// question: does a woken waiter run inside the notify, before the notifier's next statement?
// fact: notify does not run the waiter inside the call: the woken thread runs in the same frame once the notifier yields
// vs t7: same
// evidence: static: VM_Execute (0x7ed7e0) is entered only from VM_Resume (0x7f15c0, called only by VM_SetTime 0x7f1b60) and the three Scr_Exec*Thread (0x7f1970, 0x7f1a20, 0x7f1ad0): no notify path runs a waiter inside the call
// expect: f0 T0 notifying
// expect: f0 T0 after notify
// expect: f0 T1 woke
// expect: f0 T1 woke still running

main()
{
	thread waiter();
	logprint("notifying");
	level notify("go");
	logprint("after notify");
}

waiter()
{
	level waittill("go");
	logprint("woke");
	logprint("woke still running");
}
