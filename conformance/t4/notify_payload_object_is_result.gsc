// question: a notify whose first value is an existing struct: does the waiter's bound name hold that same struct?
// fact: an object sent with a notify reaches the waiter as the same object (a reference, not a copy)
// expect: f1 T0 sent
// expect: f1 T1 same=1 v=5
// vs t7: not run in T7 (its waittill binds names as here; T8: the object is the waittill result and gets _notify written into it)
// note: T4 waittill binds values to names (`waittill("e", s)`); it has no result value

main()
{
	obj = spawnstruct();
	obj.v = 5;
	thread waiter(obj);
	wait 0.05;
	level notify("e", obj);
	iprintln("sent");
}

waiter(obj)
{
	level waittill("e", s);
	iprintln("same=" + (s == obj) + " v=" + s.v);
}
