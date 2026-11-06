// question: does waittillframeend resume at the end of the current frame, after threads that do not wait?
// expect: f0 T0 main 1
// expect: f0 T0 main 2
// expect: f0 T1 frameend
// fact: waittillframeend resumes in the same frame after the threads that do not wait
#namespace zz_test;

function main()
{
	thread late();
	logprint("main 1");
	logprint("main 2");
}

function late()
{
	waittillframeend;
	logprint("frameend");
}
