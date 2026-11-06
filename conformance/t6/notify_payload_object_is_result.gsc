// question: a notify whose payload is a struct: does the waiter's bound name get that same struct?
// fact: a struct sent with a notify is bound by reference: the waiter's name is the same object
// vs t7: same: T7 binds the same way (no T7 test); T8's object payload is the waittill result itself
// expect: f1 T0 sent
// expect: f1 T1 same=1 v=5

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
