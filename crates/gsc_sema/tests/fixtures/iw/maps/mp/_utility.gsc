// Shared helpers, merged into every file that includes them.
waittill_any_timeout( timeout, a )
{
	self endon( "death" );
	wait timeout;
}

/// Prints to everyone.
announce( text )
{
	iprintln( text );
}

// Named like a builtin: builtin-first games call the builtin from other files.
spawnstruct()
{
	return 3;
}
