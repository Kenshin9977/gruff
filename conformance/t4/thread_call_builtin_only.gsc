// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: no: a threaded call never resolves to a builtin; the game's compiler refuses the script ('unknown function')
// expect compile error: unknown function
// vs t7: same (T7: the script fails to load with an unresolved external)

main()
{
	iprintln("before");
	thread spawnstruct();
	iprintln("after");
}
