// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: a threaded call never reaches a builtin: the bytecode has no threaded builtin call (the thread-call opcodes carry a script code position), and the compiler refuses the script
// expect compile error: builtin calls can't be threaded
// vs iw6: same

main()
{
	logprint("before");
	thread spawnstruct();
	logprint("after");
}
