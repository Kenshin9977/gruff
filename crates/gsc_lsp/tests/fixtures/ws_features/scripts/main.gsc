#using scripts\shapes;
#using scripts\shapes;
#using scripts\unused;
#insert scripts\macros.gsh;

#namespace main;

function init( count, name )
{
	level.callback = &on_event;
	level.total = 1;
	level.total += 2;
	sq = new cSquare();
	n = 5;
	label = "hello";
	helper( count, name );
	helper( 1, 2 );
	x = DOUBLE( count );
	self waittill( "spawned" );
	extra::far_away();
}

function helper( a, b )
{
	return a + b;
}

function on_event()
{
	wait 1;
}
