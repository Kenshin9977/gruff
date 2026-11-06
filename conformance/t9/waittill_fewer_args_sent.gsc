// question: T8 waittill binds no names: the notify sends a struct with one field, the waiter reads two: the missing one is undefined?
// fact: a field the notify's payload struct does not have reads undefined
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f1 T1 a=1 b defined=0
// expect: f1 T0 sent
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e", {#a:1});
	iprintln("sent");
}

function waiter()
{
	s = level waittill(#"e");
	iprintln("a=" + s.a + " b defined=" + isdefined(s.b));
}
