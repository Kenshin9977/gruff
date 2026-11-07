// question: a notify sent before the waittill is registered, in the same frame: is it lost?
// expect: f0 T0 notified
// expect: f0 T1 waiting
// expect: f2 T0 notified again
// expect: f2 T1 woke
// fact: a notify sent before the waittill is registered is lost, even in the same frame
// vs iw6: same

main()
{
	level notify("go");
	logprint("notified");
	thread waiter();
	wait 0.1;
	level notify("go");
	logprint("notified again");
}

waiter()
{
	logprint("waiting");
	level waittill("go");
	logprint("woke");
}
