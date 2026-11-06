// question: does a childthread stop when the thread that started it ends?
// fact: childthread is a T8 keyword (ScriptThreadCallEndOn), but the child does not stop when the thread that started it ends: it only inherits the starter's endons
// vs t7: T7: childthread is not a keyword (the official compiler reads it as an undefined local)
// evidence: static: ScriptThreadCallEndOn (0x274ac00) is ScriptThreadCall (0x274c910) plus 0x2749ac0, which walks the calling frame's thread and its parents and copies each of their endon entries (endon table 0x83114d0) onto the new thread (0x2774940); nothing ties the child to the parent's end
// expect: f0 T1 parent start
// expect: f0 T2 child tick 0
// expect: f1 T2 child tick 1
// expect: f2 T1 parent end
// expect: f2 T2 child tick 2
// expect: f3 T2 child tick 3
// expect: f4 T2 child tick 4
// expect: f5 T2 child tick 5
// expect: f6 T2 child tick 6
// expect: f7 T2 child tick 7
// expect: f8 T2 child tick 8
// expect: f9 T2 child tick 9
#namespace zz_test;

function main()
{
	thread parent();
}

function parent()
{
	iprintln("parent start");
	childthread child();
	wait 0.1;
	iprintln("parent end");
}

function child()
{
	for (i = 0; i < 10; i++)
	{
		iprintln("child tick " + i);
		wait 0.05;
	}
}
