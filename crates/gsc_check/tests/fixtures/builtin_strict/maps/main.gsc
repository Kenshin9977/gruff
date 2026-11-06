#include maps\lib;

main()
{
	thread randomint( 3 );      // B005: a threaded call never reaches a builtin
	thread lib_helper();        // fine
	thread getent( "a" );       // fine: lib's getent, a threaded call skips the builtin
	f = ::randomint;            // B005: the builtin's native handler
	g = ::lib_helper;           // fine
	h = ::spawn;                // B005: the builtin, not this file's spawn
	e = getent( "a" );          // B002: the builtin, lib's getent does not shadow it
	r = randomint( 3 );         // fine
	x = spawn();                // B002: the builtin comes before this file's spawn
	y = maps\main::spawn();     // fine: path::f reaches this file's function
}

spawn()
{
	return 1;
}
