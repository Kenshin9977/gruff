// question: t = s for a struct, then t.x is set: does s.x change (structs are references)?
// fact: structs are references
// vs t8: same
// expect: f0 T0 s.x=1 same=1
#namespace zz_test;

function set_x(obj)
{
	obj.x = 1;
}

function main()
{
	s = spawnstruct();
	t = s;
	set_x(t);
	iprintln("s.x=" + s.x + " same=" + (s == t));
}
