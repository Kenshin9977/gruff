// Constructs Black Ops III lacks, one per line where possible.
#include demo\_old_style;
#inline demo\_old_header;
#undef DEMO_FLAG
#ifdef DEMO_FLAG
#endif

DEMO_CONSTANT = 3;

function event_handler[demo_event] on_demo( eventstruct )
{
}

function uses_pointer_param( *target )
{
}

function lacks()
{
	self childthread watcher();
	call [[ level.fn ]]();
	x = a ?? b;
	y = a?.b;
	z = a is undefined;
	arr = [ 1, 2 ];
	s = { #key: 1 };
	[ first, second ] = pair();
	f = ::watcher;
	maps\demo\_util::go();
	w = thisthread;
}

old_style_function()
{
}

function fine_in_t7()
{
	waitframe();
	breakpoint;
	prof_begin( "x" );
	v = vararg;
}
