// question: an endon notify while the thread is in a timed wait: killed before the wait ends?
// fact: an endon kills a thread in a timed wait
// vs t8: same
// expect: f0 T1 waiting
// expect: f1 T0 sent stop
#namespace zz_test;

function main()
{
	thread sleeper();
	wait 0.05;
	level notify(#"stop");
	iprintln("sent stop");
	wait 0.2;
}

function sleeper()
{
	level endon(#"stop");
	iprintln("waiting");
	wait 0.1;
	iprintln("woke (should not happen)");
}
