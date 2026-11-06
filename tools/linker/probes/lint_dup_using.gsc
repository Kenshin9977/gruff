// Probe lint-dup-using: the same #using twice.
#using scripts\shared\util_shared;
#using scripts\shared\util_shared;

#namespace zz_probe;

function autoexec main()
{
	util::wait_network_frame();
}
