// question: `thread spawnstruct()` when the current namespace defines spawnstruct: the script function runs as a thread
// fact: thread f() reaches the current namespace's script function when a builtin has the same name
// vs t6: same
// expect: f0 T1 script spawnstruct called
// expect: f0 T0 after

spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

main()
{
	thread spawnstruct();
	logprint("after");
}
