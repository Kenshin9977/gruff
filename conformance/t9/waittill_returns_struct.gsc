// question: what does waittill return: a struct holding the event in _notify and the payload's fields?
// fact: waittill returns a struct: the payload's fields, and _notify holding the event (a hash)
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f1 T1 notify is e=1 a=1 b=two
// expect: f1 T0 sent
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e", {#a:1, #b:"two"});
	iprintln("sent");
}

function waiter()
{
	s = level waittill(#"e");
	iprintln("notify is e=" + (s._notify == #"e") + " a=" + s.a + " b=" + s.b);
}
