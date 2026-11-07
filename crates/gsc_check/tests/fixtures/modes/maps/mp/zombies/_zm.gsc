main()
{
	blend = 1;
	blend setsoundblend( "a", "b", 0 );   // fine: zombies have it
	a = getaiarray( "axis" );             // B001: not in zombies
}
