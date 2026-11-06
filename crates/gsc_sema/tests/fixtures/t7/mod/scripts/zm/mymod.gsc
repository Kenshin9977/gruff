#using scripts\shared\util_shared;
#using scripts\zm\mymod_helpers;
#insert scripts\shared\shared.gsh;

#namespace mymod;

class beacon : lamp
{
	function on()
	{
		toggle();
		reset_all();
	}
}

class lamp
{
	function toggle()
	{
	}
}

function autoexec main()
{
	level thread watch( 3 );
	thread util::waittill_any_timeout( 1, "a", "b" );
	s = spawnstruct();
	n = self getentitynumber();
	sys::spawnstruct();
	if ( IS_TRUE( level.ready ) )
	{
		helper_in_namespace();
	}
	helper_from_using();
	f = &watch;
	g = ::watch;
	nobody_defines_this();
	earthquake( 1, 2, self.origin, 100 );
	playfx( 0, "fx" );
	b = new beacon();
	[[ b ]]->toggle();
}

function watch( count )
{
	self endon( "death" );
	foreach ( player in getplayers() )
	{
		total = count + player.score;
	}
	self waittill( "done", who );
	return who.health;
}

function spawnstruct()
{
	return 2;
}
