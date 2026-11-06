#using scripts\helpers;

#insert scripts\macros.gsh;

#namespace demo;

function init()
{
	e = spwan( "script_origin", ( 0, 0, 0 ) );
	n = randomint();
	ok = randomint( 10 );
	self iprintlnbold( "hello" );
	self setmodel();
	notamethod();
	self getplayers();
	playfx( "fx", ( 0, 0, 0 ), ( 0, 0, 1 ) );
	self thread custom_spawn( 1, 2, 3 );
	[[ level.callback ]]( 1 );
	x = function_1a2b3c4d( 1 );
	y = custom_spawn( 1, 2, 3 );
	IS_GOOD( y );
	helpers::wait_a_bit();
	line( ( 0, 0, 0 ), ( 1, 1, 1 ) );
	/# line( ( 0, 0, 0 ), ( 1, 1, 1 ) ); #/
	t = gettime();
	totally_unknown_thing( 1 );
}

function custom_spawn( a, b, c )
{
	return a;
}

function spin_forever()
{
	while ( 1 )
	{
		self.counter++;
	}
}

function spin_for()
{
	for ( ;; )
	{
		self.counter++;
		thread helpers::wait_a_bit();
	}
}

function waits_directly()
{
	while ( true )
	{
		self waittill( "damage" );
	}
}

function waits_through_call()
{
	while ( 1 )
	{
		helpers::wait_a_bit();
	}
}

function waits_through_macro()
{
	while ( 1 )
	{
		WAIT_FRAME;
	}
}

function calls_pure()
{
	while ( 1 )
	{
		helpers::pure();
	}
}

function exits()
{
	while ( 1 )
	{
		if ( self.counter > 10 )
		{
			break;
		}
		self.counter++;
	}
}

function unknown_callee()
{
	while ( 1 )
	{
		not_defined_anywhere();
	}
}

function suppressed()
{
	while ( 1 ) // gsc: ignore[L001]
	{
		self.counter++;
	}
	// gsc: ignore[infinite-loop-without-wait]
	while ( 1 )
	{
		self.counter++;
	}
	wait 1; // gsc: ignore[L001]
	wait 1; // gsc: ignore[B001, Q123]
	n = randomint(); // gsc: ignore[B002, L001]
}
