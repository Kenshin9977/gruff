// question: does a woken waiter run inside the notify, before the notifier's next statement?
// fact: notify runs the woken waiter inside the call: the waiter runs to its next wait or its end before the notifier's next statement
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// evidence: static: the Notify handler (0x1b70290) calls VM_Notify (0x1bb5b10), which for each woken waiter calls VM_Resume (0x1bb76f0, then VM_Execute 0x1bb0df0) at once: the waiter runs inside the notify, before the notifier's next statement
// expect: f0 T0 notifying
// expect: f0 T1 woke
// expect: f0 T1 woke still running
// expect: f0 T0 after notify
#namespace zz_test;

function main()
{
	thread waiter();
	iprintln("notifying");
	level notify(#"go");
	iprintln("after notify");
}

function waiter()
{
	level waittill(#"go");
	iprintln("woke");
	iprintln("woke still running");
}
