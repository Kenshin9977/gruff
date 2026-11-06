// question: two threads that wait the same time: do they resume in the order they started waiting?
// fact: threads whose waits end in the same frame resume in reverse order of their wait calls
// vs t6: same
// expect: f1 T3 c
// expect: f1 T2 b
// expect: f1 T1 a

main()
{
	thread w("a");
	thread w("b");
	thread w("c");
}

w(name)
{
	wait 0.05;
	logprint(name);
}
