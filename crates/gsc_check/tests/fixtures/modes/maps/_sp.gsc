main()
{
	fx = spawnfx( 1, 2, 3, 4, 5, 6 );     // fine: this file's spawnfx, no campaign builtin has the name
	playfx( 1, 2, 3, 4, 5, 6 );           // fine: the campaign's playfx takes 2 to 6
	a = getaiarray( "axis" );             // fine: a campaign builtin
	r = randomint( 1, 2 );                // B002
}

spawnfx( a, b, c, d, e, f )
{
	return 1;
}
