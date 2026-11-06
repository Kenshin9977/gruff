// question: does the VM stop a loop that never waits, and how?
// expect: f0 T0 before
// expect: f0 T0 script error: potential infinite loop in script - killing thread.
// fact: the VM has a wall-clock loop guard: a loop with no wait that runs 2.5 s (timeGetTime, from the execution's start) is stopped at its next jump back; retail (abort_on_error 0) kills the looping thread with 'potential infinite loop in script - killing thread.' (its next statement never runs, the VM goes on); developer 1 (abort_on_error 1) raises it as a terminal script error (Com_Error 'script runtime error')
// evidence: static: OP_jumpback (opcode 0x63, handler 0x51a3d9) compares the wall clock since Scr_ResetTimeout with 2500 ms (0x51a403); past it: under the script debugger a warning and a reset (0x51a42a), else with abort_on_error (0x178c06d) clear the 'killing thread' print and the unwind (0x51a49e), else 'potential infinite loop in script' with terminal_error set (0x51a573). Unlike T4, whose Scr_Error (0x69ab70) stores its terminal argument 0 over the flag, IW3 keeps it terminal (developer trace)
// evidence: harness: timeGetTime is a virtual clock (+1 ms per read, +50 ms per frame): the iteration count is the harness's, the outcome the game's
// vs t4: same

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
