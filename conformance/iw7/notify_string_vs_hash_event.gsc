// question: is an event sent as a string the same event as its hash?
// unsupported: IW7 has no hash literals (#"..."): events are strings only; the compiler (gsc-tool) refuses '#'
// expect compile error: unexpected token '#'
// evidence: the compiler is gsc-tool (IW7 ships no compiler: its scripts are bytecode compiled offline)
// vs s1: same

main()
{
	thread hash_waiter();
	thread string_waiter();
	wait 0.05;
	level notify("go");
	logprint("sent string");
	wait 0.05;
	level notify(#"back");
	logprint("sent hash");
}

hash_waiter()
{
	level waittill(#"go");
	logprint("hash waiter woke");
}

string_waiter()
{
	level waittill("back");
	logprint("string waiter woke");
}
