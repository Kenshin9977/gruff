// Probe lint-macro-arity: a function-like macro called with the wrong argument count.
#namespace zz_probe;

#define M( a ) ( a + 1 )

function autoexec main()
{
	x = M( 1, 2 );
}
