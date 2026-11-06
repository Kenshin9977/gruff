// question: an endon fired while the thread waits on another event: is the thread killed?
// fact: an endon kills a thread blocked in waittill on another event
// vs t7: same
// expect: f0 T1 waiting
// expect: f1 T0 sent stop
// expect: f1 T0 sent go
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"stop");
	iprintln("sent stop");
	level notify(#"go");
	iprintln("sent go");
}

function waiter()
{
	level endon(#"stop");
	iprintln("waiting");
	level waittill(#"go");
	iprintln("woke (should not happen)");
}
