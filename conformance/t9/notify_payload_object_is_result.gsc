// question: a notify whose payload is an existing struct: is the waiter's result that same struct, with _notify written into it?
// fact: a notify whose payload is an object hands that same object to the waiter as the waittill result, with _notify written into it; the waiter runs inside the notify
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f1 T1 same=1 v=5
// expect: f1 T0 sent, obj._notify is e=1
#namespace zz_test;

function main()
{
	obj = spawnstruct();
	obj.v = 5;
	thread waiter(obj);
	wait 0.05;
	level notify(#"e", obj);
	iprintln("sent, obj._notify is e=" + (obj._notify === #"e"));
}

function waiter(obj)
{
	s = level waittill(#"e");
	iprintln("same=" + (s == obj) + " v=" + s.v);
}
