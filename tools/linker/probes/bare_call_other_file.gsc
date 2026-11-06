// Probe t7-bare-call: a bare call to a function that only another namespace defines.
// probe-with: zz_probe_lib.gsc
#using scripts\zz_harness\zz_probe_lib;
#namespace zz_probe;

function autoexec main()
{
	a = zz_lib_only();
	b = zz_probe_lib::zz_lib_only();
}
