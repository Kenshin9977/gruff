// question: waittill with two events wakes on either, and _notify tells which
// fact: waittill with several events wakes on the first of them, and the result's _notify is the event that came
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f1 T1 woke on b=1
// expect: f1 T0 sent b
// expect: f2 T0 sent a
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"b");
	iprintln("sent b");
	wait 0.05;
	level notify(#"a");
	iprintln("sent a");
}

function waiter()
{
	s = level waittill(#"a", #"b");
	iprintln("woke on b=" + (s._notify == #"b"));
}
