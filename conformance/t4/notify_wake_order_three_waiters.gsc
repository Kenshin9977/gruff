// question: three waiters registered in order a, b, c on one event: wake order?
// fact: waiters on one event wake in reverse order of registration (last registered first)
// expect: f1 T0 notifying
// expect: f1 T3 c
// expect: f1 T2 b
// expect: f1 T1 a
// vs t7: same

main()
{
	thread waiter("a");
	thread waiter("b");
	thread waiter("c");
	wait 0.05;
	iprintln("notifying");
	level notify("go");
}

waiter(name)
{
	level waittill("go");
	iprintln(name);
}
