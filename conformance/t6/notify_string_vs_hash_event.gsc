// question: can an event be a hash literal: does waittill(#"go") wake on notify("go"), and notify(#"back") wake waittill("back")?
// fact: events must be strings: waittill or notify with a hash is a non-terminal script error ('first parameter of waittill/notify must evaluate to a string') and the statement is skipped: the waiter runs on at once, the notify is not sent
// vs t7: same: T7 events are strings only (no T7 test)
// evidence: static: both messages are raised in VM_Execute (0x7f0cdf, 0x7f0cf7)
// expect: f0 T1 script error: first parameter of waittill must evaluate to a string
// expect: f0 T1 hash waiter woke
// expect: f1 T0 sent string
// expect: f2 T0 script error: first parameter of notify must evaluate to a string
// expect: f2 T0 sent hash

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
	wait 0.05;
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
