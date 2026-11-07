// question: same as endon_vs_waittill_same_event, with the endon registered in another thread order: the waiter waits first, a second thread of the same self registers no endon
// expect: f0 T1 waiting
// expect: f1 T0 sent
// expect: f1 T1 woke
// fact: control: without the endon the waiter wakes
// vs iw5: same

main()
{
	thread waiter();
	wait 0.05;
	level notify("e");
	logprint("sent");
}

waiter()
{
	logprint("waiting");
	level waittill("e");
	logprint("woke");
}
