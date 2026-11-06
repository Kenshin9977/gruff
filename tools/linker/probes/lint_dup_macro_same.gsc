// Probe lint-dup-macro-same: a macro defined twice with the same body.
#namespace zz_probe;

#define N 1
#define N 1

function autoexec main()
{
	x = N;
}
