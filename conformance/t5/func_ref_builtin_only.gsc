// question: `::spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or an error?
// fact: a function reference never reaches a builtin: the game's compiler refuses ::spawnstruct with no script function ('unknown function')
// vs t6: the T6 linker resolves it to the builtin's native handler, which then runs as bytecode
// expect compile error: unknown function

main()
{
	logprint("before");
	f = ::spawnstruct;
	s = [[ f ]]();
	logprint("called, struct=" + isdefined(s));
}
