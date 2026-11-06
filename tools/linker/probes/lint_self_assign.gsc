// Probe lint-self-assign: an assignment to self.
#namespace zz_probe;

function autoexec main()
{
	self = level;
}
