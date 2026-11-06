// question: in which order do two waiters on the same event wake?
// fact: the last registered waiter wakes first, and both run inside the notify, before the notifier's next statement
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// evidence: static: the Notify handler (0x1b70290) calls VM_Notify (0x1bb5b10), which for each woken waiter calls VM_Resume (0x1bb76f0, then VM_Execute 0x1bb0df0) at once: the waiter runs inside the notify, before the notifier's next statement
// expect: f1 T0 notifying
// expect: f1 T2 second
// expect: f1 T1 first
// expect: f1 T0 after notify
#namespace zz_test;

function main()
{
	thread waiter("first");
	thread waiter("second");
	wait 0.05;
	iprintln("notifying");
	level notify(#"go");
	iprintln("after notify");
}

function waiter(name)
{
	level waittill(#"go");
	iprintln(name);
}
