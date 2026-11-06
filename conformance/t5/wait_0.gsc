// question: does wait 0 yield and resume in the same frame, the next one, or is it refused?
// fact: wait 0 resumes in the same frame, before threads in waittillframeend
// vs t6: same
// expect: f0 T0 before
// expect: f0 T0 after wait 0
// expect: f0 T1 other

main()
{
	thread other();
	logprint("before");
	wait 0;
	logprint("after wait 0");
}

other()
{
	waittillframeend;
	logprint("other");
}
