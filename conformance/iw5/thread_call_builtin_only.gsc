// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: a threaded call never reaches a builtin: the bytecode has no threaded builtin call (the thread-call opcodes 0x24-0x27 and 0x2b-0x2e carry a script code position), and the compiler refuses the script
// evidence: static: VM_ExecuteInternal's thread-call cases (0x24-0x27, 0x2b-0x2e) jump to a code position read from the operand (0x2a3c7f); builtins are reached only by the call-builtin opcodes; the compiler is gsc-tool
// expect compile error: builtin calls can't be threaded
// vs iw3: same

main()
{
	logprint("before");
	thread spawnstruct();
	logprint("after");
}
