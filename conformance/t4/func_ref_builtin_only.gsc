// question: `::spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or an error?
// fact: a function reference never falls back to a builtin: the game's compiler refuses the script ('unknown function')
// expect compile error: unknown function
// vs t7: T7 falls back to the builtin (&spawnstruct works and calling it runs the builtin)

main()
{
	iprintln("before");
	f = ::spawnstruct;
	s = [[ f ]]();
	iprintln("called, struct=" + isdefined(s));
}
