#include maps\_utility;

main()
{
	level.x=getent( "x","targetname" );
	thread loop ( level.x );
}

loop(ent)
{
	ent endon("death");
	while( 1 )
	{
		ent waittill( "trigger",who );
		if(isPlayer(who))
			who iprintln( &"GAME_HELLO" );
	}
}
