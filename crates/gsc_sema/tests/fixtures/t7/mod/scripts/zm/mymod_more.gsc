#namespace mymod;

function helper_in_namespace()
{
	watch( 2 );
	// mymod::spawnstruct (another file, same namespace) comes before the builtin.
	return spawnstruct();
}
