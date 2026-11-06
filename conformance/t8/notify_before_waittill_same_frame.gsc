// question: a notify sent before the waittill is registered, in the same frame: is it lost?
// fact: a notify sent before the waittill is registered is lost, even in the same frame
// vs t7: same
// expect: f0 T0 notified
// expect: f0 T1 waiting
// expect: f2 T0 notified again
// expect: f2 T1 woke
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
