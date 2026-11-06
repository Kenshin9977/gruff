// question: does the VM stop a loop that never waits, and how?
// expect: f0 T0 before
// fact: VM_Execute has no loop guard: a loop with no wait runs until the instruction budget (20M) of the harness
// expect error: instruction limit
// evidence: static: the Jump handler (0x12cd9b0) only moves pos; VM_Execute's loop (0x12edb30) counts opcodes into a profiling counter (0x5124820, read by the frame stats at 0x19e1c30) and checks only its exit flag; the build has no 'infinite loop' string
#namespace zz_test;

function main()
{
	logprint("before");
	i = 0;
	for (;;)
	{
		i++;
	}
	logprint("after");
}
