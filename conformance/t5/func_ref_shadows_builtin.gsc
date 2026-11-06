// question: `::spawnstruct` when the file defines spawnstruct: the script function or the builtin?
// fact: the script function of the file
// vs t6: T6: the builtin (its native handler run as bytecode)
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 result=42

spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

main()
{
	f = ::spawnstruct;
	r = [[ f ]]();
	logprint("result=" + r);
}
