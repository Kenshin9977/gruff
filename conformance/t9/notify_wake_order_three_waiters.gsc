// question: three waiters registered in order a, b, c on one event: wake order?
// fact: waiters wake in reverse order of registration (c, b, a), inside the notify
// vs t8: same order; T8 runs them after the notifier yields, T9 inside the notify
// evidence: static: the Notify handler (0x1b70290) calls VM_Notify (0x1bb5b10), which for each woken waiter calls VM_Resume (0x1bb76f0, then VM_Execute 0x1bb0df0) at once: the waiter runs inside the notify, before the notifier's next statement
// expect: f1 T0 notifying
// expect: f1 T3 c
// expect: f1 T2 b
// expect: f1 T1 a
#namespace zz_test;

function main()
{
	thread waiter("a");
	thread waiter("b");
	thread waiter("c");
	wait 0.05;
	iprintln("notifying");
	level notify(#"go");
}

function waiter(name)
{
	level waittill(#"go");
	iprintln(name);
}
