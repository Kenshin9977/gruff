// Fixture written for gsc_index tests.
#using scripts\shared\relay;
#using scripts\shared\missing_file;
#insert scripts\shared\flags.gsh;

#namespace beacon;

/@
"Name: start( <id>, [delay] )"
"Summary: Lights the beacon and waits for it to burn out."
@/
function autoexec start( id, delay = 1 )
{
	level endon( "game_over" );
	self thread watch_lamp( id );
	relay::send( "lit_" + id );
	wait delay;
	level notify( "beacon_lit", id, self );
	level.beacons[ id ] = self;
	level.beacon_count++;
	if ( IS_ON( self.ready ) )
	{
		self.state = "burning";
	}
	self setmodel( "beacon_on" );
	wait TICK;
}

/// Waits for the lamp to break.
/// Then reports it.
function private watch_lamp( id )
{
	self endon( "death", "disconnect" );
	self util::waittill_any_timeout( 5, "lamp_broken", "lamp_removed" );
	self waittill( "trigger_" + id + "_hit", attacker );
	msg = self.next_msg;
	self waittill( msg );
	self notify( "lamp_out" );
	callback = level.on_lamp_out;
	self [[ callback ]]( id );
	player = getplayers()[ 0 ];
	player childthread relay::on_out( id );
	player waittill( "spawned_player" );
	nobody_defines_this();
	unknown_ns::thing();
	/#
		relay::debug_draw();
	#/
}

/#
function dev_only()
{
	level notify( "trigger_7_hit" );
}
#/

function extinguish( id )
{
	BURN_OUT( id );
}
