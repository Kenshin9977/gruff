#include maps\mp\_utility;

main()
{
	announce( "dm" );
	maps\mp\_utility::waittill_any_timeout( 1, "x" );
	maps\mp\_missing::nothing();
	s = spawnstruct();
	thread onplayerconnect();
	level.callback = ::onplayerconnect;
	not_included();
}

onplayerconnect()
{
	self waittill( "spawned_player" );
}
