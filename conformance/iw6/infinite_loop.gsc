// question: does the VM stop a loop that never waits, and how?
// fact: the VM has a wall-clock loop guard: a loop with no wait that runs 5 s (Sys_Milliseconds since the execution started) is stopped at its next jump back: the looping thread is killed silently (no error, no text in this build; its next statement never runs) and the VM goes on
// evidence: harness: timeGetTime is a virtual clock (+1 ms per read, +50 ms per frame): the iteration count is the harness's, the outcome the game's
// vs iw5: same
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
