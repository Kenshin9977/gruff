// question: a woken waiter that notifies a second event: does the second waiter run before the first notifier resumes?
// fact: a notify sent by a woken waiter is deferred the same way: each woken thread runs after the current one yields
// expect: f0 T0 notify a
// expect: f0 T0 after notify a
// expect: f0 T1 got a
// expect: f0 T1 after notify b
// expect: f0 T2 got b
// vs t7: same

main()
{
	thread first();
	thread second();
	iprintln("notify a");
	level notify("a");
	iprintln("after notify a");
}

first()
{
	level waittill("a");
	iprintln("got a");
	level notify("b");
	iprintln("after notify b");
}

second()
{
	level waittill("b");
	iprintln("got b");
}
