// question: does `thread f()` run f at once, up to its first wait, before the caller goes on?
// fact: thread f() runs f at once up to its first wait, then the caller continues
// vs t7: same
// expect: f0 T0 before
// expect: f0 T1 child start
// expect: f0 T0 after call
// expect: f1 T1 child after wait
#namespace zz_test;

function main()
{
	iprintln("before");
	thread child();
	iprintln("after call");
}

function child()
{
	iprintln("child start");
	wait 0.05;
	iprintln("child after wait");
}
