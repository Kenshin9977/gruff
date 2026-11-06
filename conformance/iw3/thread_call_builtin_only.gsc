// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: no: a threaded call never resolves to a builtin; the game's compiler refuses the script ('unknown function')
// expect compile error: unknown function
// vs t4: same

main()
{
	logprint("before");
	thread spawnstruct();
	logprint("after");
}
