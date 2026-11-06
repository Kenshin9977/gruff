// question: `thread spawnstruct()` when the current namespace defines spawnstruct: the script function runs as a thread
// expect: f0 T1 script spawnstruct called
// expect: f0 T0 after
// fact: thread f() reaches the current namespace's script function when a builtin has the same name
// vs t7: same

spawnstruct()
{
	iprintln("script spawnstruct called");
	return 42;
}

main()
{
	thread spawnstruct();
	iprintln("after");
}
