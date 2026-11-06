// question: does waittilltimeout exist in T5?
// vs t6: same: T6's linker reports it unresolved
// unsupported: the form does not exist in T5: no; the game's own compiler refuses the script at load
// expect compile error: unknown function

main()
{
	logprint("start");
	level waittilltimeout(0.1, "never");
	logprint("returned");
}
