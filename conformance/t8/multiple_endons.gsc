// question: two endons on one thread: does either event kill it?
// fact: each of several endons kills the thread
// vs t7: same
// expect: f0 T1 a waiting
// expect: f0 T2 b waiting
// expect: f1 T0 sent second
#namespace zz_test;

function main()
{
	thread waiter("a");
	thread waiter("b");
	wait 0.05;
	level notify(#"second");
	iprintln("sent second");
	wait 0.05;
	level notify(#"go");
}

function waiter(name)
{
	level endon(#"first");
	level endon(#"second");
	iprintln(name + " waiting");
	level waittill(#"go");
	iprintln(name + " woke");
}
