#include maps\mp\_zm_utility;

init()
{
	players = getplayers();
	if ( is_player_valid( players[ 0 ] ) )
	{
		// Includes do not chain: `shared_check` is in a file `_zm_utility` includes.
		shared_check( players[ 0 ] );
	}
	maps\mp\_zm_shared::shared_check( players[ 0 ] );
}
