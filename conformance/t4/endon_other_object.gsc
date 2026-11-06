// question: an endon on a struct is not fired by the same event on level (and the reverse)
// expect: f0 T1 waiting
// expect: f1 T0 level sent
// expect: f2 T0 struct sent
// fact: an endon is bound to its object: the same event on another object does not kill
// vs t7: same

main()
{
	s = spawnstruct();
	level.s = s;
	thread waiter(s);
	wait 0.05;
	level notify("stop");
	iprintln("level sent");
	wait 0.05;
	s notify("stop");
	iprintln("struct sent");
	wait 0.05;
	level notify("go");
}

waiter(s)
{
	s endon("stop");
	iprintln("waiting");
	level waittill("go");
	iprintln("woke (only if alive)");
}
