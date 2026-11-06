#using scripts\shared\util_shared;
#using scripts\shared\other_shared;

#define REGISTER(f) function autoexec register_it() { level.f = f; }

// File scope, before the first #namespace: the default namespace (the file name).
REGISTER( &early_init )

function early_init()
{
}

#namespace mymod;

function main()
{
	wait_network_frame();              // B004: only util::wait_network_frame exists
	util::wait_network_frame();        // fine
	self thread wait_network_frame();  // B004
	f = &wait_network_frame;           // B004
	in_two_namespaces();               // B004, no fix: two namespaces define it
	e = spawn( "script_origin" );      // fine: the builtin
	g = &spawn;                        // fine: no mymod::spawn, so the builtin
	thread randomint( 3 );             // B005: a thread never reaches a builtin
	thread local_helper();             // fine
	self thread setmodel( "x" );       // B005
	nobody_defines_this();             // B001 territory, not B004
	/# wait_network_frame(); #/        // not checked in a dev block
}

function local_helper()
{
	randomint( 3 );
}
