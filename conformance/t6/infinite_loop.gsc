// question: does the VM stop a loop that never waits, and how?
// fact: VM_Execute has no loop guard: a loop with no wait runs until the instruction budget (20M) of the harness
// vs t7: same
// evidence: static: the build has no 'infinite loop' string; the jump opcodes only move pos
// expect: f0 T0 before
// expect error: instruction limit

main()
{
	logprint("before");
	i = 0;
	for (;;)
	{
		i++;
	}
	logprint("after");
}
