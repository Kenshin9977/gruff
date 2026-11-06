// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: a threaded call never resolves to a builtin: the script fails to load with an unresolved external
// vs t7: same
// evidence: static: the linker's thread case (kind 3, 0x7e0a86) calls only the script resolver 0x7df5f0
// expect error: Unresolved external :  "spawnstruct" with 0 parameters

main()
{
	logprint("before");
	thread spawnstruct();
	logprint("after");
}
