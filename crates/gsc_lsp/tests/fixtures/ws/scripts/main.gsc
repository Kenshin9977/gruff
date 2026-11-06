#using scripts\util;

#namespace main;

function init( count )
{
	total = util::helper( count, 2 );
	self.health = total;
	ent = spawn( "script_model", ( 0, 0, 0 ) );
	thread spin();
}

function spin()
{
	while ( true )
	{
		n = randomint( 10 );
	}
}

function   messy(  a  ){
return a;}
