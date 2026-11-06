#if DEMO_VERSION >= 2
function setup( a, b, c )
{
#else // older builds
function setup( a, b )
{
	c = undefined;
#endif
	return a + b;
}
