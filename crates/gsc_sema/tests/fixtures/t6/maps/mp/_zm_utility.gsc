#include maps\mp\_zm_shared;

is_player_valid( player )
{
	return isdefined( player ) && shared_check( player );
}
