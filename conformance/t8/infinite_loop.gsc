// question: does the VM stop a loop that never waits, and how?
// fact: VM_Execute has no loop guard: a loop with no wait runs until the harness budget (20M instructions)
// vs t7: same
// evidence: static: VM_Execute (0x276ece0) is a bare fetch/dispatch loop; nothing in it counts instructions
// expect: f0 T0 before
// expect error: instruction limit
#namespace zz_test;

function main()
{
	iprintln("before");
	i = 0;
	for (;;)
	{
		i++;
	}
	iprintln("after");
}
