// question: does a thread with no wait run to its end before the caller resumes?
// fact: a thread with no wait runs to its end before the caller continues
// vs t7: same
// expect: f0 T0 before
// expect: f0 T1 child 1
// expect: f0 T1 child 2
// expect: f0 T0 after call
#namespace zz_test;

function main()
{
	iprintln("before");
	thread child();
	iprintln("after call");
}

function child()
{
	iprintln("child 1");
	iprintln("child 2");
}
