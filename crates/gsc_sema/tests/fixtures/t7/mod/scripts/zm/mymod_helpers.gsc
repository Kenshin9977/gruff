#using scripts\shared\util_shared;

#namespace mymod_helpers;

function helper_from_using()
{
	mymod::watch( 1 );
	// The builtin comes before util::spawnstruct of the #using'd file.
	return spawnstruct();
}

function earthquake()
{
}
