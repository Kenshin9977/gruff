// Fixture written for gsc_index tests.
#namespace relay;

function send( msg )
{
	level notify( msg );
	level waittill( "beacon_lit", id, owner );
}

function register()
{
	level.on_lamp_out = &on_out;
	level.on_ready = &beacon::start;
	thread send( "relay_ready" );
	helper();
}

function on_out( id )
{
	self waittillmatch( "anim_done", "end" );
	self util::delay_notify( "relay_timeout", 2 );
}

function debug_draw()
{
}

class lamp
{
	var lit;

	constructor()
	{
		lit = false;
	}

	function toggle()
	{
		lit = !lit;
		self notify( "toggled" );
		reset();
	}

	function reset()
	{
	}
}

class strong_lamp : lamp
{
	function toggle()
	{
		lamp::toggle();
		reset();
	}
}

function use_lamp( l )
{
	[[ l ]]->toggle();
}
