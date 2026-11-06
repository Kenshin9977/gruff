// Probe lint-dup-macro-param: a macro parameter repeated.
#namespace zz_probe;

#define M( a, a ) ( a + 1 )

function autoexec main()
{
	x = M( 1, 2 );
}
