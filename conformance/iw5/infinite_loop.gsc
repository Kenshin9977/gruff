// question: does the VM stop a loop that never waits, and how?
// fact: the VM has a wall-clock loop guard: a loop with no wait that runs 5 s (Sys_Milliseconds since the execution started) is stopped at its next jump back: the looping thread is killed silently (no error, no text in this build; its next statement never runs) and the VM goes on
// evidence: static: OP_jumpback (0x4e, 0x2a46d1) compares the time since VM_Execute's start (0x31da064) with 5000 ms; past it, with the script debugger off (0x31da060 = 0) it unwinds every frame of the thread (0x2a4734), else it resets the start time and goes on
// evidence: harness: timeGetTime is a virtual clock (+1 ms per read, +50 ms per frame): the iteration count is the harness's, the outcome the game's
// vs iw3: IW3 stops it after 2.5 s with 'potential infinite loop in script - killing thread.'; IW5 after 5 s, silently
// expect: f0 T0 before
// expect: f0 T0 script error: loop guard: thread killed

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
