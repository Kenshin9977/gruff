// question: in which order do two waiters on the same event wake?
// fact: two waiters: the last registered wakes first, both after the notifier yields
// expect: f1 T0 notifying
// expect: f1 T0 after notify
// expect: f1 T2 second
// expect: f1 T1 first
// evidence: static: as notify_wakes_waiter_before_notifier_continues; the order comes from the time list
#namespace zz_test;

function main()
{
	thread waiter("first");
	thread waiter("second");
	wait 0.05;
	logprint("notifying");
	level notify("go");
	logprint("after notify");
}

function waiter(name)
{
	level waittill("go");
	logprint(name);
}
