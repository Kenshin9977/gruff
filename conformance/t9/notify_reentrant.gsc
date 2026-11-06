// question: a woken waiter that notifies a second event: does the second waiter run before the first notifier resumes?
// fact: a notify sent by a woken waiter also runs its own waiter at once: the calls nest (got a, got b, after notify b, after notify a)
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// evidence: static: the Notify handler (0x1b70290) calls VM_Notify (0x1bb5b10), which for each woken waiter calls VM_Resume (0x1bb76f0, then VM_Execute 0x1bb0df0) at once: the waiter runs inside the notify, before the notifier's next statement
// expect: f0 T0 notify a
// expect: f0 T1 got a
// expect: f0 T2 got b
// expect: f0 T1 after notify b
// expect: f0 T0 after notify a
#namespace zz_test;

function main()
{
	thread first();
	thread second();
	iprintln("notify a");
	level notify(#"a");
	iprintln("after notify a");
}

function first()
{
	level waittill(#"a");
	iprintln("got a");
	level notify(#"b");
	iprintln("after notify b");
}

function second()
{
	level waittill(#"b");
	iprintln("got b");
}
