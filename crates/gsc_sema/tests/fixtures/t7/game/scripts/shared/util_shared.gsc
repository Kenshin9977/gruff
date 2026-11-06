#insert scripts\shared\shared.gsh;

#namespace util;

/@
"Summary: Waits for one of the notifies, or the timeout"
"MandatoryArg: <timeout> seconds"
@/
function waittill_any_timeout( timeout, a, b )
{
	self endon( "death" );
	wait timeout;
	return a;
}

/// Starts the util system.
function autoexec init()
{
	level.util_ready = true;
	spawnstruct();
}

// A script function named like a builtin: bare calls from other namespaces still reach the
// builtin first.
function spawnstruct()
{
	return 1;
}

function private hidden()
{
}
