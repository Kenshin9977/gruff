#using scripts\demo\common_util;
#using scripts/demo/other_util;

#insert scripts\demo\demo.gsh;

#namespace demo_ns;

#precache( "model", "demo_box" );
#using_animtree( "demo_tree" );

#define MAX_COUNT 8
#define SQUARE(x) ((x) * (x))
#define LONG_ONE(a, b) \
	(a + \
	 b) // trailing comment

REGISTER_DEMO( "demo", &init, undefined )

function init()
{
}
