// question: the notify sends three plain values after the event: what does the waiter's result struct hold?
// fact: plain values after the event are dropped: the waiter still wakes, with a struct that only has _notify
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f1 T1 struct=1 notify is e=1
// expect: f1 T0 sent
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e", 1, 2, 3);
	iprintln("sent");
}

function waiter()
{
	s = level waittill(#"e");
	iprintln("struct=" + isdefined(s) + " notify is e=" + (s._notify == #"e"));
}
