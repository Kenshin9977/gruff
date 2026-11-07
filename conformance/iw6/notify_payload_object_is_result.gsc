// question: a notify whose first value is an existing struct: does the waiter's bound name hold that same struct?
// fact: an object sent with a notify reaches the waiter as the same object (a reference, not a copy)
// expect: f1 T0 sent
// expect: f1 T1 same=1 v=5
// vs iw5: same
// note: IW6 waittill binds values to names (`waittill("e", s)`); it has no result value

main()
{
	obj = spawnstruct();
	obj.v = 5;
	thread waiter(obj);
	wait 0.05;
	level notify("e", obj);
	logprint("sent");
}

waiter(obj)
{
	level waittill("e", s);
	logprint("same=" + (s == obj) + " v=" + s.v);
}
