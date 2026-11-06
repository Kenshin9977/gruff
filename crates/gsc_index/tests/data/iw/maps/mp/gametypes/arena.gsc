// Fixture written for gsc_index tests (include-style game).
#include maps\mp\_helpers;

ROUND_LIMIT = 3;

main()
{
	level.callbackplayerconnect = ::on_player_connect;
	thread watch_rounds();
	x = get_max( 1, 2 );
	maps\mp\_helpers::safe_wait();
	playfx( level._effect[ "spark" ], ( 0, 0, 0 ) );
}

on_player_connect()
{
	self endon( "disconnect" );
	for ( ;; )
	{
		self waittill( "spawned_player" );
		self notify( "arena_ready" );
	}
}

watch_rounds()
{
	level waittill( "round_" + level.round + "_over" );
	[[ level.callbackplayerconnect ]]();
}
