// Probe lint-private-call: a call to another namespace private function.
// probe-with: zz_probe_private.gsc
#using scripts\zz_harness\zz_probe_private;
#namespace zz_probe;

function autoexec main()
{
	zz_probe_private::hidden();
}
