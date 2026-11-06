// Probe lint-div-zero: division by a literal zero.
#namespace zz_probe;

function autoexec main()
{
	x = 1;
	y = x / 0;
}
