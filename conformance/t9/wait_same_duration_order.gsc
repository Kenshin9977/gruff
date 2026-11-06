// question: two threads that wait the same time: do they resume in the order they started waiting?
// fact: waits ending in the same frame resume in the order of the wait calls (first in, first out)
// vs t8: same
// expect: f1 T1 a
// expect: f1 T2 b
// expect: f1 T3 c
#namespace zz_test;

function main()
{
	thread w("a");
	thread w("b");
	thread w("c");
}

function w(name)
{
	wait 0.05;
	iprintln(name);
}
