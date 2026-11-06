#include maps\lib;

main()
{
	s = spawnstruct();       // builtin: an #include'd function does not shadow it
	n = lib_only();          // #include'd
	p = getplayers();        // the same file's function shadows the builtin
	thread spawnstruct();    // #include'd: a threaded call skips the builtin
	thread iprintln( "x" );  // unresolved: a threaded call never reaches a builtin
	f = ::lib_only;          // #include'd
	g = ::iprintln;          // unresolved: a reference never reaches a builtin
	h = ::getplayers;        // the same file
}

getplayers()
{
	return 1;
}
