// question: does the VM stop a loop that never waits, and how?
// expect: f0 T0 before
// expect: f0 T0 script error: potential infinite loop in script - killing thread.
// fact: the VM has a wall-clock loop guard: a loop with no wait that runs 2.5 s (timeGetTime, from the execution's start) is stopped at its next jump back; retail (abort_on_error 0) kills the looping thread with 'potential infinite loop in script - killing thread.' (its next statement never runs, the VM goes on); developer 1 (abort_on_error 1) raises it as a terminal script error (Com_Error 'script runtime error')
// vs t7: T7 has no loop guard (the loop runs until the harness budget)
// evidence: static: OP_jumpback (opcode 0x63, handler 0x6962b0) compares timeGetTime minus the time of the last Scr_ResetTimeout (0x69ae60) with 2500 ms (0x6962e8); past it: under the script debugger a warning and a reset (0x696318), else with scrVmPub.abort_on_error clear the 'killing thread' print and the stack unwind (0x69639c), else Scr_Error 'potential infinite loop in script' as terminal (0x696486)
// evidence: harness: timeGetTime is a virtual clock (+1 ms per read, +50 ms per frame), so the 2.5 s are counted in reads (one per jump back): the iteration count is the harness's, the outcome the game's. With timeGetTime stubbed to 0, as first written, the guard never fired and the run looked like T7's (no guard): a fake agreement

main()
{
	iprintln("before");
	i = 0;
	for (;;)
	{
		i++;
	}
	iprintln("after");
}
