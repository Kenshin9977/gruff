// question: a notify sent before the waittill is registered, in the same frame: is it lost?
// fact: a notify sent before the waittill is registered is lost, even in the same frame; the later notify runs the waiter inside the call
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f0 T0 notified
// expect: f0 T1 waiting
// expect: f2 T1 woke
// expect: f2 T0 notified again
#namespace zz_test;

function main()
{
	level notify(#"go");
	iprintln("notified");
	thread waiter();
	wait 0.1;
	level notify(#"go");
	iprintln("notified again");
}

function waiter()
{
	iprintln("waiting");
	level waittill(#"go");
	iprintln("woke");
}
