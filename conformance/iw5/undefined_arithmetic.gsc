// question: what does undefined + 1 do at run time: an error that ends the thread, or a value?
// fact: undefined + 1 is a non-terminal script error (raised by the add opcode 0x5b): the thread goes on and the result is undefined
// evidence: static: the binary-operator cases longjmp to VM_ExecuteInternal's recovery (setjmp 0x2a288d, landing 0x2a519f), which pushes undefined and resumes the thread; this build keeps no error text
// vs iw3: same (IW3's message: pair 'undefined' and '1' has unmatching types 'undefined' and 'int')
// expect: f0 T0 before
// expect: f0 T0 script error: OP_plus (opcode 0x5b)
// expect: f0 T0 after 0

main()
{
	logprint("before");
	x = undefined;
	y = x + 1;
	logprint("after " + isdefined(y));
}
