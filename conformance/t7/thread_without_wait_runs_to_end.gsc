// question: does a thread with no wait run to its end before the caller resumes?
// expect: f0 T0 before
// expect: f0 T1 child 1
// expect: f0 T1 child 2
// expect: f0 T0 after call
// fact: a thread with no wait runs to its end before the caller continues
#namespace zz_test;

function main()
{
	logprint("before");
	thread child();
	logprint("after call");
}

function child()
{
	logprint("child 1");
	logprint("child 2");
}
