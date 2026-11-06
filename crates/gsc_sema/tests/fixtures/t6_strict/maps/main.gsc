#include maps\lib;

main()
{
	s = spawnstruct();             // builtin: before every script function
	n = lib_only();                // #include'd
	p = getplayers();              // builtin, although this file defines getplayers
	q = maps\main::getplayers();   // the same file: path::f skips the builtins
	thread getplayers();           // the same file: a threaded call skips the builtins
	thread spawnstruct();          // #include'd
	thread iprintln( "x" );        // unresolved: a threaded call never reaches a builtin
	f = ::lib_only;                // #include'd
	g = ::iprintln;                // builtin: the game stores its native handler
	h = ::getplayers;              // builtin, not the same file's function
	i = getentitynumber();         // the same file: the builtin is a method
	j = self getentitynumber();    // builtin method
}

getentitynumber()
{
	return 2;
}

getplayers()
{
	return 1;
}
