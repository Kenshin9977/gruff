// question: a waittill on a struct is not woken by the same event on level, and the reverse
// expect: f0 T1 struct waiter
// expect: f0 T2 level waiter
// expect: f1 T0 level sent
// expect: f1 T2 level woke
// expect: f2 T0 struct sent
// expect: f2 T1 struct woke
// fact: waittill on an object is not woken by the same event on another object
// vs iw6: same

main()
{
	s = spawnstruct();
	thread on_struct(s);
	thread on_level();
	wait 0.05;
	level notify("e");
	logprint("level sent");
	wait 0.05;
	s notify("e");
	logprint("struct sent");
}

on_struct(s)
{
	logprint("struct waiter");
	s waittill("e");
	logprint("struct woke");
}

on_level()
{
	logprint("level waiter");
	level waittill("e");
	logprint("level woke");
}
