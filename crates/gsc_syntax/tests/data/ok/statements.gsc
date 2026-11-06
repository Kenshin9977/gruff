function flow( n )
{
	if ( n == 0 )
		return;
	else if ( n == 1 )
	{
		wait 0.05;
	}
	else if ( n == 2 )
		waitrealtime 1;
	else
		waittillframeend;

	while ( n > 0 )
		n--;

	do
	{
		n++;
	}
	while ( n < 3 );

	foreach ( key, value in level.things )
	{
		if ( !isdefined( value ) )
			continue;
		break;
	}

	switch ( n )
	{
		case 0:
		case "one":
			n = 1;
			break;
		default:
			n = 2;
	}

	const LIMIT = 4;
	for ( ;; )
	{
		WAIT_A_FRAME
		DEMO_MACRO( n )
	}
	/#
	println( "dev only" );
	#/
	;
}
