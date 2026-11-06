// Probe lint-dup-macro: a macro defined twice.
#namespace zz_probe;

#define N 1
#define N 2

function autoexec main()
{
	x = N;
}
