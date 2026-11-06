// Fixture written for gsc_index tests: the client side of relay.
#using scripts\shared\relay;

#namespace relay;

function autoexec client_init()
{
	level waittill( "relay_ready" );
	playfx( 0, "fx_relay" );
}
