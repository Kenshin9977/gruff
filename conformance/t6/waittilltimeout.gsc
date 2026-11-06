// question: does waittilltimeout exist in T6?
// vs t7: same: T7 has no waittilltimeout either (no T7 test)
// unsupported: T6 has no such builtin: the game's linker refuses the script at load (unresolved external)
// expect error: Unresolved external :  "waittilltimeout" with 2 parameters

main()
{
	logprint("start");
	level waittilltimeout(0.1, "never");
	logprint("returned");
}
