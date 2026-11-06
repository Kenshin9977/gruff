// question: a childthread started after `level endon(#"stop")`: is it killed by that event, while a plain thread started the same way is not?
// fact: a childthread inherits the endons active in the thread that starts it: the starter's endon event kills the child too, while a plain thread started at the same point keeps running
// vs t8: same
// expect: f0 T2 child tick 0
// expect: f0 T3 plain tick 0
// expect: f1 T2 child tick 1
// expect: f1 T3 plain tick 1
// expect: f1 T0 sent stop
// expect: f2 T3 plain tick 2
#namespace zz_test;

function main()
{
	thread parent();
	wait 0.05;
	level notify(#"stop");
	iprintln("sent stop");
	wait 0.1;
}

function parent()
{
	level endon(#"stop");
	childthread ticker("child");
	thread ticker("plain");
	wait 1;
	iprintln("parent end (should not happen)");
}

function ticker(name)
{
	for (i = 0; i < 3; i++)
	{
		iprintln(name + " tick " + i);
		wait 0.05;
	}
}
