// question: endon with two events: does the second one kill the thread?
// fact: endon with two events: either one kills the thread
// vs t8: same
// expect: f0 T1 waiting
// expect: f1 T0 sent second
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"second");
	iprintln("sent second");
	wait 0.05;
	level notify(#"go");
}

function waiter()
{
	level endon(#"first", #"second");
	iprintln("waiting");
	level waittill(#"go");
	iprintln("woke (should not happen)");
}
