// question: `&spawnstruct` when the current namespace defines spawnstruct: the script function or the builtin?
// fact: &f reaches the current namespace's function first
// vs t7: same
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 result=42
#namespace zz_test;

function spawnstruct()
{
	iprintln("script spawnstruct called");
	return 42;
}

function main()
{
	f = &spawnstruct;
	r = [[ f ]]();
	iprintln("result=" + r);
}
