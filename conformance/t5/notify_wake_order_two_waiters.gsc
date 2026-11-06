// question: in which order do two waiters on the same event wake?
// fact: two waiters: the last registered wakes first, both after the notifier yields
// vs t6: same
// expect: f1 T0 notifying
// expect: f1 T0 after notify
// expect: f1 T2 second
// expect: f1 T1 first

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
