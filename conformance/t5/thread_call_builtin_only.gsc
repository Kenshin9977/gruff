// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: a threaded call never resolves to a builtin: the game's compiler refuses `thread spawnstruct()` with no script function ('unknown function')
// vs t6: same answer; T6 refuses it at link time (unresolved external)
// expect compile error: unknown function

main()
{
	logprint("before");
	thread spawnstruct();
	logprint("after");
}
