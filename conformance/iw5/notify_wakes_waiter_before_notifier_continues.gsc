// question: does a woken waiter run inside the notify, before the notifier's next statement?
// fact: notify does not run the waiter inside the call: VM_Notify only queues it (ScrVar_SetWaitTime to the current time); it runs in the same frame once the notifier yields
// expect: f0 T0 notifying
// expect: f0 T0 after notify
// expect: f0 T1 woke
// expect: f0 T1 woke still running
// vs iw3: same

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
