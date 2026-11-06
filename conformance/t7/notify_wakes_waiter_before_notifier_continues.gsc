// question: does a woken waiter run inside the notify, before the notifier's next statement?
// fact: notify does not run the waiter inside the call: VM_Notify only queues it (ScrVar_SetWaitTime to the current time); it runs in the same frame once the notifier yields
// expect: f0 T0 notifying
// expect: f0 T0 after notify
// expect: f0 T1 woke
// expect: f0 T1 woke still running
// evidence: static: the Notify handler (0x12d3f70) calls VM_Notify (0x12d36c0), whose body calls ScrVar_SetWaitTime (0x12e0c20) and never VM_Resume (0x12ee230, called only by the run-for-time function 0x12ee430) or VM_Execute
#namespace zz_test;

function main()
{
	thread waiter();
	logprint("notifying");
	level notify("go");
	logprint("after notify");
}

function waiter()
{
	level waittill("go");
	logprint("woke");
	logprint("woke still running");
}
