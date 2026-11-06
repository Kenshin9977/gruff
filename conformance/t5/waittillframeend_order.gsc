// question: does waittillframeend resume at the end of the current frame, after threads that do not wait?
// fact: waittillframeend resumes in the same frame after the threads that do not wait
// vs t6: same
// expect: f0 T0 main 1
// expect: f0 T0 main 2
// expect: f0 T1 frameend

main()
{
	thread late();
	logprint("main 1");
	logprint("main 2");
}

late()
{
	waittillframeend;
	logprint("frameend");
}
