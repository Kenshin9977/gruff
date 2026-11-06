#include maps\lib;

main()
{
	thread randomint( 3 );      // B005: a threaded call never reaches a builtin
	thread lib_helper();        // fine
	thread getent( "a" );       // fine: lib's getent, a threaded call skips the builtin
	f = ::randomint;            // B005: a reference never reaches a builtin
	g = ::lib_helper;           // fine
	e = getent( "a" );          // B002: the builtin, lib's getent does not shadow it
	r = randomint( 3 );         // fine
	x = spawn();                // fine: this file's spawn shadows the builtin
}

spawn()
{
	return 1;
}
