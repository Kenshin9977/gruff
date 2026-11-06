// question: does `thread f()` run f at once, up to its first wait, before the caller goes on?
// expect: f0 T0 before
// expect: f0 T1 child start
// expect: f0 T0 after call
// expect: f1 T1 child after wait
// fact: thread f() runs f at once until its first wait, then the caller continues
#namespace zz_test;

function main()
{
	logprint("before");
	thread child();
	logprint("after call");
}

function child()
{
	logprint("child start");
	wait 0.05;
	logprint("child after wait");
}
