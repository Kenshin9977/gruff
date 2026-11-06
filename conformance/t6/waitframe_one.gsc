// question: does waitframe exist in T6?
// vs t7: same: T7 has no waitframe either (no T7 test)
// unsupported: T6 has no such builtin: the game's linker refuses the script at load (unresolved external)
// expect error: Unresolved external :  "waitframe" with 1 parameters

main()
{
	logprint("start");
	waitframe(1);
	logprint("after waitframe 1");
	wait 0.05;
	logprint("after wait 0.05");
}
