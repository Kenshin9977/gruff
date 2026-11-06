// question: does waitframe exist in T5?
// vs t6: same: T6's linker reports it unresolved
// unsupported: the form does not exist in T5: no; the game's own compiler refuses the script at load
// expect compile error: unknown function

main()
{
	logprint("start");
	waitframe(1);
	logprint("after waitframe 1");
	wait 0.05;
	logprint("after wait 0.05");
}
