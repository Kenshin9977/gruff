// question: an endon on a struct is not fired by the same event on level (and the reverse)
// expect: f0 T1 waiting
// expect: f1 T0 level sent
// expect: f2 T0 struct sent
// fact: an endon is bound to its object: the same event on another object does not kill
// vs iw3: same

main()
{
	s = spawnstruct();
	level.s = s;
	thread waiter(s);
	wait 0.05;
	level notify("stop");
	logprint("level sent");
	wait 0.05;
	s notify("stop");
	logprint("struct sent");
	wait 0.05;
	level notify("go");
}

waiter(s)
{
	s endon("stop");
	logprint("waiting");
	level waittill("go");
	logprint("woke (only if alive)");
}
