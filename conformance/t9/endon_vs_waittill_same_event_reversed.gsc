// question: same as endon_vs_waittill_same_event, with the endon registered in another thread order: the waiter waits first, a second thread of the same self registers no endon
// fact: control: without the endon the waiter wakes, inside the notify
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f0 T1 waiting
// expect: f1 T1 woke
// expect: f1 T0 sent
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e");
	iprintln("sent");
}

function waiter()
{
	iprintln("waiting");
	level waittill(#"e");
	iprintln("woke");
}
