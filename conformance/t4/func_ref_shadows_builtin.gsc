// question: `&spawnstruct` when the current namespace defines spawnstruct: the script function or the builtin?
// fact: a function reference resolves to the current namespace's script function first
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 result=42
// vs t7: same

spawnstruct()
{
	iprintln("script spawnstruct called");
	return 42;
}

main()
{
	f = ::spawnstruct;
	r = [[ f ]]();
	iprintln("result=" + r);
}
