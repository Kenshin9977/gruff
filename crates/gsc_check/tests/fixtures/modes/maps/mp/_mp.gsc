main()
{
	playfx( 1, 2, 3, 4, 5, 6 );           // B002: the multiplayer playfx takes 2 to 4
	a = getaiarray( "axis" );             // B001: a campaign builtin only
	v = vectorcross( 1, 2 );              // the builtin, not the function below
	e = spawnfx( 1, 2 );                  // fine
}

vectorcross( a, b )                       // B006
{
	return a;
}
