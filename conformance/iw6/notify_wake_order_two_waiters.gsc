// question: in which order do two waiters on the same event wake?
// fact: two waiters: the last registered wakes first, both after the notifier yields
// expect: f1 T0 notifying
// expect: f1 T0 after notify
// expect: f1 T2 second
// expect: f1 T1 first
// vs iw5: same

main()
{
	thread waiter("first");
	thread waiter("second");
	wait 0.05;
	logprint("notifying");
	level notify("go");
	logprint("after notify");
}

waiter(name)
{
	level waittill("go");
	logprint(name);
}
