// question: `thread spawnstruct()` when no script function spawnstruct exists: does a threaded call fall back to the builtin?
// fact: thread spawnstruct() with no script function: the script does not load (a threaded call never falls back to a builtin; fatal Sys_Error)
// vs t7: same
// claim: acts-t8-link (agrees): a thread import never falls back to a builtin
// expect error: Sys_Error 0x6394f836
#namespace zz_test;

function main()
{
	iprintln("before");
	thread spawnstruct();
	iprintln("after");
}
