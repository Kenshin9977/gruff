#include demo\_shared_util;
#using_animtree( "demo_tree" );

DEMO_SPEED = 120;

main()
{
	maps\demo\_helper::start();
	self childthread watch();
	level.ptr = ::watch;
	[[ level.ptr ]]();
	ratio = (float)self.hits / (int)( level.total + 1 );
}

watch()
{
	self endon( "death" );
}
