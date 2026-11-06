// question: a notify nobody waits for: no error, the caller goes on?
// fact: a notify with no waiter is silent
// vs t7: same
// expect: f0 T0 before
// expect: f0 T0 after

main()
{
	logprint("before");
	level notify("nobody");
	logprint("after");
}
