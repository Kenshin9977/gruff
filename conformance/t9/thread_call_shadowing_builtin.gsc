// question: `thread spawnstruct()` when the current namespace defines spawnstruct: the script function runs as a thread
// fact: thread f() reaches the current namespace's function when a builtin has the same name
// vs t8: same
// expect: f0 T1 script spawnstruct called
// expect: f0 T0 after
#namespace zz_test;

function spawnstruct()
{
	iprintln("script spawnstruct called");
	return 42;
}

function main()
{
	thread spawnstruct();
	iprintln("after");
}
