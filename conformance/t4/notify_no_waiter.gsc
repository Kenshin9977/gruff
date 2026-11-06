// question: a notify nobody waits for: no error, the caller goes on?
// expect: f0 T0 before
// expect: f0 T0 after
// fact: a notify with no waiter is silent
// vs t7: same

main()
{
	iprintln("before");
	level notify("nobody");
	iprintln("after");
}
