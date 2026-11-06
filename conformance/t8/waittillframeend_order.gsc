// question: does waittillframeend resume at the end of the current frame, after threads that do not wait?
// fact: waittillframeend resumes in the same frame after threads that do not wait
// vs t7: same
// expect: f0 T0 main 1
// expect: f0 T0 main 2
// expect: f0 T1 frameend
#namespace zz_test;

function main()
{
	thread late();
	iprintln("main 1");
	iprintln("main 2");
}

function late()
{
	waittillframeend();
	iprintln("frameend");
}
