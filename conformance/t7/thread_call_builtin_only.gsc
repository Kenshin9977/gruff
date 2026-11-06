// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: no: a threaded call never resolves to a builtin; the script fails to load with an unresolved external
// claim: acts-t8-link (agrees): thread imports never fall back to builtins (acts' BO4 linker)
// expect error: Unresolved external "A8FB77BD"
#namespace zz_test;

function main()
{
	logprint("before");
	thread spawnstruct();
	logprint("after");
}
