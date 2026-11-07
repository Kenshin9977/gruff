// question: a thread that both endon()s and waittill()s the same event: woken or killed?
// expect: f0 T1 waiting
// expect: f1 T0 sent
// fact: a thread that endon()s and waittill()s the same event is killed by it, not woken
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
	level endon("e");
	logprint("waiting");
	level waittill("e");
	logprint("woke");
}
