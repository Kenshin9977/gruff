// question: what does undefined + 1 do at run time: an error that ends the thread, or a value?
// fact: undefined + 1 is a non-terminal script error (raised by the add opcode 0x46): the thread goes on and the result is undefined
// vs s1: same (only the opcode numbering differs)
// expect: f0 T0 before
// expect: f0 T0 script error: OP_plus (opcode 0x46)
// expect: f0 T0 after 0

main()
{
	logprint("before");
	x = undefined;
	y = x + 1;
	logprint("after " + isdefined(y));
}
