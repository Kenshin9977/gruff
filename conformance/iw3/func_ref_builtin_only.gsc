// question: `::spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or an error?
// fact: a function reference never falls back to a builtin: the game's compiler refuses the script ('unknown function')
// expect compile error: unknown function
// vs t4: same

main()
{
	logprint("before");
	f = ::spawnstruct;
	s = [[ f ]]();
	logprint("called, struct=" + isdefined(s));
}
