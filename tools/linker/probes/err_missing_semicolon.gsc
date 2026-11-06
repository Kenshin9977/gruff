// Probe harness: a syntax error the linker must reject with a line and column.
#namespace zz_probe;

function autoexec main()
{
	x = 1
	y = x + 2;
}
