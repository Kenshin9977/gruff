// question: does a woken waiter run inside the notify, before the notifier's next statement?
// fact: notify does not run the waiter inside the call: the woken thread runs in the same frame once the notifier yields
// vs t6: same
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
